//! User preference profile repository (`user_preference_profiles` table,
//! migration 0044).
//!
//! B1 用户偏好画像事实源（ADR-0025 §2.4 Slice 3）。全局唯一偏好存储：
//! B1 沉淀流程写入，K8（PromptVersion 反思引擎）与 B2（项目层演进）只读。
//! 访问应通过 `evolution::PreferenceService::snapshot()` 接口，不直接调用。

use rusqlite::{params, Connection, OptionalExtension};

use crate::store::StoreError;

/// Asset category — the scope of a preference entry (ADR-0025 §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferenceCategory {
    /// Cross-project user convention (written to ~/.nuomi/rules/).
    Convention,
    /// User preference knowledge fact (written to memory_entries).
    Memory,
    /// Engineering skill or script (written to .nuomi/skills/).
    Skill,
}

impl PreferenceCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Convention => "convention",
            Self::Memory => "memory",
            Self::Skill => "skill",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "convention" => Some(Self::Convention),
            "memory" => Some(Self::Memory),
            "skill" => Some(Self::Skill),
            _ => None,
        }
    }
}

/// Lifecycle status of a preference entry (ADR-0025 §2.3 软撤回).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferenceStatus {
    /// Active preference, in effect.
    Confirmed,
    /// Soft-undone within the 30 s window (or manually revoked via Compass).
    Revoked,
}

impl PreferenceStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Revoked => "revoked",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "revoked" => Self::Revoked,
            _ => Self::Confirmed,
        }
    }
}

/// A `user_preference_profiles` row.
#[derive(Debug, Clone, PartialEq)]
pub struct UserPreferenceProfile {
    pub id: String,
    pub category: PreferenceCategory,
    pub key: String,
    pub content: String,
    /// 0.0 ~ 1.0.
    pub confidence: f64,
    /// M5 parent FK to sessions(id).
    pub source_session_id: Option<String>,
    pub status: PreferenceStatus,
    pub created_at: i64,
    pub updated_at: i64,
    /// unix-ms; `None` until status becomes `revoked`.
    pub revoked_at: Option<i64>,
}

/// Inserts a new preference profile row at status `confirmed`.
pub fn insert(conn: &Connection, rec: &UserPreferenceProfile) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO user_preference_profiles
           (id, category, key, content, confidence, source_session_id,
            status, created_at, updated_at, revoked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            rec.id,
            rec.category.as_str(),
            rec.key,
            rec.content,
            rec.confidence,
            rec.source_session_id,
            rec.status.as_str(),
            rec.created_at,
            rec.updated_at,
            rec.revoked_at,
        ],
    )?;
    Ok(())
}

/// Fetches a row by id.
pub fn get(conn: &Connection, id: &str) -> Result<Option<UserPreferenceProfile>, StoreError> {
    conn.query_row(
        "SELECT id, category, key, content, confidence, source_session_id,
                status, created_at, updated_at, revoked_at
         FROM user_preference_profiles WHERE id = ?1",
        [id],
        row_to_profile,
    )
    .optional()
    .map_err(Into::into)
}

/// Lists all confirmed entries, ordered by confidence descending.
pub fn list_confirmed(conn: &Connection) -> Result<Vec<UserPreferenceProfile>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, category, key, content, confidence, source_session_id,
                status, created_at, updated_at, revoked_at
         FROM user_preference_profiles
         WHERE status = 'confirmed'
         ORDER BY confidence DESC",
    )?;
    let rows = stmt.query_map([], row_to_profile)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Lists confirmed entries by category.
pub fn list_by_category(
    conn: &Connection,
    category: PreferenceCategory,
) -> Result<Vec<UserPreferenceProfile>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, category, key, content, confidence, source_session_id,
                status, created_at, updated_at, revoked_at
         FROM user_preference_profiles
         WHERE status = 'confirmed' AND category = ?1
         ORDER BY confidence DESC",
    )?;
    let rows = stmt.query_map([category.as_str()], row_to_profile)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Lists confirmed entries with confidence ≥ `min_confidence`.
pub fn list_high_confidence(
    conn: &Connection,
    min_confidence: f64,
) -> Result<Vec<UserPreferenceProfile>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, category, key, content, confidence, source_session_id,
                status, created_at, updated_at, revoked_at
         FROM user_preference_profiles
         WHERE status = 'confirmed' AND confidence >= ?1
         ORDER BY confidence DESC",
    )?;
    let rows = stmt.query_map(params![min_confidence], row_to_profile)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Soft-revokes a preference entry (ADR-0025 §2.3). Sets status to `revoked`
/// and stamps `revoked_at`. Returns `true` if a row was updated.
pub fn revoke(conn: &Connection, id: &str, now_ms: i64) -> Result<bool, StoreError> {
    let n = conn.execute(
        "UPDATE user_preference_profiles
         SET status = 'revoked', updated_at = ?2, revoked_at = ?2
         WHERE id = ?1 AND status = 'confirmed'",
        params![id, now_ms],
    )?;
    Ok(n > 0)
}

/// Updates the confidence of a confirmed entry.
pub fn update_confidence(
    conn: &Connection,
    id: &str,
    confidence: f64,
    now_ms: i64,
) -> Result<bool, StoreError> {
    let n = conn.execute(
        "UPDATE user_preference_profiles
         SET confidence = ?2, updated_at = ?3
         WHERE id = ?1 AND status = 'confirmed'",
        params![id, confidence, now_ms],
    )?;
    Ok(n > 0)
}

/// Counts total confirmed entries (for budget/quota checks).
pub fn count_confirmed(conn: &Connection) -> Result<i64, StoreError> {
    conn.query_row(
        "SELECT count(*) FROM user_preference_profiles WHERE status = 'confirmed'",
        [],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

fn row_to_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<UserPreferenceProfile> {
    let category_str: String = row.get(1)?;
    let status_str: String = row.get(6)?;
    Ok(UserPreferenceProfile {
        id: row.get(0)?,
        category: PreferenceCategory::parse(&category_str).unwrap_or(PreferenceCategory::Memory),
        key: row.get(2)?,
        content: row.get(3)?,
        confidence: row.get(4)?,
        source_session_id: row.get(5)?,
        status: PreferenceStatus::parse(&status_str),
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
        revoked_at: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    fn sample_profile(id: &str, key: &str) -> UserPreferenceProfile {
        UserPreferenceProfile {
            id: id.to_string(),
            category: PreferenceCategory::Convention,
            key: key.to_string(),
            content: "prefer no unwrap in Rust".to_string(),
            confidence: 0.85,
            source_session_id: None,
            status: PreferenceStatus::Confirmed,
            created_at: 1000,
            updated_at: 1000,
            revoked_at: None,
        }
    }

    #[test]
    fn insert_and_get_round_trip() {
        let conn = fresh_db();
        let rec = sample_profile("p1", "rust.no_unwrap");
        insert(&conn, &rec).unwrap();
        let got = get(&conn, "p1").unwrap().unwrap();
        assert_eq!(got.key, "rust.no_unwrap");
        assert_eq!(got.category, PreferenceCategory::Convention);
        assert_eq!(got.status, PreferenceStatus::Confirmed);
        assert!((got.confidence - 0.85).abs() < 1e-9);
    }

    #[test]
    fn list_confirmed_orders_by_confidence_desc() {
        let conn = fresh_db();
        let mut low = sample_profile("p1", "a");
        low.confidence = 0.3;
        let mut high = sample_profile("p2", "b");
        high.confidence = 0.9;
        insert(&conn, &low).unwrap();
        insert(&conn, &high).unwrap();
        let list = list_confirmed(&conn).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].confidence >= list[1].confidence);
    }

    #[test]
    fn revoke_marks_revoked_and_stamps_time() {
        let conn = fresh_db();
        insert(&conn, &sample_profile("p1", "k")).unwrap();
        assert!(revoke(&conn, "p1", 2000).unwrap());
        let got = get(&conn, "p1").unwrap().unwrap();
        assert_eq!(got.status, PreferenceStatus::Revoked);
        assert_eq!(got.revoked_at, Some(2000));
        // Re-revoke is a no-op.
        assert!(!revoke(&conn, "p1", 3000).unwrap());
    }

    #[test]
    fn list_by_category_filters() {
        let conn = fresh_db();
        let mut conv = sample_profile("p1", "a");
        conv.category = PreferenceCategory::Convention;
        let mut mem = sample_profile("p2", "b");
        mem.category = PreferenceCategory::Memory;
        insert(&conn, &conv).unwrap();
        insert(&conn, &mem).unwrap();
        let convs = list_by_category(&conn, PreferenceCategory::Convention).unwrap();
        assert_eq!(convs.len(), 1);
        let mems = list_by_category(&conn, PreferenceCategory::Memory).unwrap();
        assert_eq!(mems.len(), 1);
    }

    #[test]
    fn list_high_confidence_filters_by_threshold() {
        let conn = fresh_db();
        let mut low = sample_profile("p1", "a");
        low.confidence = 0.5;
        let mut high = sample_profile("p2", "b");
        high.confidence = 0.9;
        insert(&conn, &low).unwrap();
        insert(&conn, &high).unwrap();
        let result = list_high_confidence(&conn, 0.8).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].key, "b");
    }

    #[test]
    fn update_confidence_changes_value() {
        let conn = fresh_db();
        insert(&conn, &sample_profile("p1", "k")).unwrap();
        assert!(update_confidence(&conn, "p1", 0.95, 2000).unwrap());
        let got = get(&conn, "p1").unwrap().unwrap();
        assert!((got.confidence - 0.95).abs() < 1e-9);
        assert_eq!(got.updated_at, 2000);
    }

    #[test]
    fn count_confirmed_excludes_revoked() {
        let conn = fresh_db();
        insert(&conn, &sample_profile("p1", "a")).unwrap();
        insert(&conn, &sample_profile("p2", "b")).unwrap();
        assert_eq!(count_confirmed(&conn).unwrap(), 2);
        revoke(&conn, "p1", 2000).unwrap();
        assert_eq!(count_confirmed(&conn).unwrap(), 1);
    }
}
