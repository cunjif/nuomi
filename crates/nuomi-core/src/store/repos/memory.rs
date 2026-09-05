//! Long-term memory repository (keyword / structured retrieval; vector later).

use rusqlite::{params, Connection};

use crate::domain::MemoryEntry;
use crate::store::StoreError;

pub fn insert(conn: &Connection, m: &MemoryEntry) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO memory_entries
         (id, content, source_session_id, tags, kind, user_profile, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            m.id,
            m.content,
            m.source_session_id,
            serde_json::to_string(&m.tags)?,
            m.kind,
            m.user_profile as i64,
            m.created_at,
            m.updated_at
        ],
    )?;
    Ok(())
}

/// Keyword search across content with optional tag filter.
/// Matches are ranked by recency (newest first).
pub fn search(
    conn: &Connection,
    keyword: Option<&str>,
    tag: Option<&str>,
    limit: u32,
) -> Result<Vec<MemoryEntry>, StoreError> {
    let mut sql = String::from(
        "SELECT id, content, source_session_id, tags, kind, user_profile, created_at, updated_at
         FROM memory_entries WHERE 1=1",
    );
    if keyword.is_some() {
        sql.push_str(" AND content LIKE '%' || ?1 || '%'");
    }
    if tag.is_some() {
        sql.push_str(if keyword.is_some() {
            " AND tags LIKE '%' || ?2 || '%'"
        } else {
            " AND tags LIKE '%' || ?1 || '%'"
        });
    }
    sql.push_str(match (keyword.is_some(), tag.is_some()) {
        (true, true) => " ORDER BY created_at DESC LIMIT ?3",
        (true, false) | (false, true) => " ORDER BY created_at DESC LIMIT ?2",
        (false, false) => " ORDER BY created_at DESC LIMIT ?1",
    });

    let kw = keyword.unwrap_or_default();
    let tg = tag.unwrap_or_default();
    let mut stmt = conn.prepare(&sql)?;
    let map_rows =
        |row: &rusqlite::Row<'_>| -> rusqlite::Result<MemoryEntry> { row_to_memory(row) };
    let rows = match (keyword, tag) {
        (Some(_), Some(_)) => stmt.query_map(params![kw, tg, limit], map_rows)?,
        (Some(_), None) => stmt.query_map(params![kw, limit], map_rows)?,
        (None, Some(_)) => stmt.query_map(params![tg, limit], map_rows)?,
        (None, None) => stmt.query_map(params![limit], map_rows)?,
    };
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// A memory search result carrying its FTS5 relevance rank (lower = better;
/// `0.0` when produced by the LIKE fallback path).
pub struct MemorySearchHit {
    pub entry: MemoryEntry,
    pub rank: f64,
}

/// Full-text search over `memory_fts` (trigram-indexed `memory_entries`).
/// Queries shorter than 3 characters fall back to the LIKE-based `search`
/// because the trigram tokenizer cannot index them. MATCH special characters
/// are neutralized by double-quoting each token.
pub fn search_fts(
    conn: &Connection,
    query: &str,
    limit: u32,
) -> Result<Vec<MemorySearchHit>, StoreError> {
    let trimmed = query.trim();
    let Some(fts_query) = build_fts_query(trimmed) else {
        return like_fallback(conn, trimmed, limit);
    };
    let mut stmt = conn.prepare(
        "SELECT m.id, m.content, m.source_session_id, m.tags, m.kind, m.user_profile,
                m.created_at, m.updated_at, memory_fts.rank
         FROM memory_fts JOIN memory_entries m ON m.rowid = memory_fts.rowid
         WHERE memory_fts MATCH ?1 ORDER BY memory_fts.rank LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![fts_query, limit], |row| {
        let entry = row_to_memory(row)?;
        Ok(MemorySearchHit {
            entry,
            rank: row.get(8)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// LIKE-based fallback ranked at 0.0 (used for sub-trigram queries).
fn like_fallback(
    conn: &Connection,
    query: &str,
    limit: u32,
) -> Result<Vec<MemorySearchHit>, StoreError> {
    Ok(search(conn, Some(query), None, limit)?
        .into_iter()
        .map(|entry| MemorySearchHit { entry, rank: 0.0 })
        .collect())
}

/// Builds a safe FTS5 MATCH expression: each whitespace-separated token is
/// double-quoted (internal quotes doubled) so special characters cannot alter
/// the query grammar. Tokens shorter than 3 characters are dropped because the
/// trigram tokenizer cannot match them. Returns `None` when nothing qualifies,
/// signaling the LIKE fallback.
fn build_fts_query(query: &str) -> Option<String> {
    let tokens: Vec<String> = query
        .split_whitespace()
        .filter(|t| t.chars().count() >= 3)
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

/// All user-profile facts (always injected by evolution).
pub fn list_user_profile(conn: &Connection) -> Result<Vec<MemoryEntry>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, content, source_session_id, tags, kind, user_profile, created_at, updated_at
         FROM memory_entries WHERE user_profile = 1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], row_to_memory)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn row_to_memory(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryEntry> {
    let tags: String = row.get(3)?;
    Ok(MemoryEntry {
        id: row.get(0)?,
        content: row.get(1)?,
        source_session_id: row.get(2)?,
        tags: crate::store::json_col(0, &tags)?,
        kind: row.get(4)?,
        user_profile: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    fn entry(id: &str, content: &str, tags: &[&str], profile: bool, at: i64) -> MemoryEntry {
        MemoryEntry {
            id: id.into(),
            content: content.into(),
            source_session_id: None,
            tags: tags.iter().map(|s| s.to_string()).collect(),
            kind: "note".into(),
            user_profile: profile,
            created_at: at,
            updated_at: at,
        }
    }

    #[test]
    fn keyword_search_matches_content_only() {
        let conn = db();
        insert(&conn, &entry("m1", "loves rust", &["lang"], false, 1)).unwrap();
        insert(&conn, &entry("m2", "prefers python", &["lang"], false, 2)).unwrap();
        insert(
            &conn,
            &entry("m3", "rust tags mention", &["python"], false, 3),
        )
        .unwrap();
        let hits = search(&conn, Some("rust"), None, 10).unwrap();
        let ids: Vec<&str> = hits.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["m3", "m1"]); // recency order
    }

    #[test]
    fn tag_filter_and_profile_listing() {
        let conn = db();
        insert(&conn, &entry("a", "x", &["work"], false, 1)).unwrap();
        insert(&conn, &entry("b", "name is james", &["me"], true, 2)).unwrap();
        insert(&conn, &entry("c", "y", &["work"], false, 3)).unwrap();
        assert_eq!(search(&conn, None, Some("work"), 10).unwrap().len(), 2);
        let profile = list_user_profile(&conn).unwrap();
        assert_eq!(profile.len(), 1);
        assert_eq!(profile[0].id, "b");
        assert!(profile[0].user_profile);
    }

    #[test]
    fn empty_search_returns_all_recency_first() {
        let conn = db();
        insert(&conn, &entry("old", "x", &[], false, 1)).unwrap();
        insert(&conn, &entry("new", "y", &[], false, 2)).unwrap();
        let all = search(&conn, None, None, 10).unwrap();
        assert_eq!(
            all.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["new", "old"]
        );
    }

    fn db_fts() -> Connection {
        // 0005_memory_fts is applied by the migrations runner itself.
        db()
    }

    #[test]
    fn fts_matches_cjk_substring_and_english() {
        let conn = db_fts();
        insert(
            &conn,
            &entry("m1", "糯米是插件化的Agent内核", &[], false, 1),
        )
        .unwrap();
        insert(&conn, &entry("m2", "loves rust programming", &[], false, 2)).unwrap();
        let cjk: Vec<String> = search_fts(&conn, "插件化", 10)
            .unwrap()
            .into_iter()
            .map(|h| h.entry.id)
            .collect();
        assert_eq!(cjk, vec!["m1"]);
        let en: Vec<String> = search_fts(&conn, "rust", 10)
            .unwrap()
            .into_iter()
            .map(|h| h.entry.id)
            .collect();
        assert_eq!(en, vec!["m2"]);
    }

    #[test]
    fn fts_short_query_falls_back_to_like() {
        let conn = db_fts();
        insert(
            &conn,
            &entry("m1", "糯米是插件化的Agent内核", &[], false, 1),
        )
        .unwrap();
        let hits = search_fts(&conn, "糯米", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].entry.id, "m1");
        assert_eq!(hits[0].rank, 0.0);
    }

    #[test]
    fn fts_special_characters_do_not_break_match() {
        let conn = db_fts();
        insert(&conn, &entry("m1", "loves rust programming", &[], false, 1)).unwrap();
        // MATCH grammar metacharacters must be neutralized, not panic.
        let hits = search_fts(&conn, "rust\" OR (1=1) NEAR --", 10).unwrap();
        assert!(hits.is_empty());
        // FTS keywords (AND/OR/NOT/NEAR) are matched literally, not parsed:
        // unquoted "programming NOT" would be a syntax error.
        let hits = search_fts(&conn, "programming NOT", 10).unwrap();
        assert!(hits.is_empty());
        let hits = search_fts(&conn, "programming", 10).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn fts_index_syncs_on_update_and_delete() {
        let conn = db_fts();
        insert(&conn, &entry("m1", "alpha beta gamma", &[], false, 1)).unwrap();
        conn.execute(
            "UPDATE memory_entries SET content = 'delta epsilon zeta' WHERE id = 'm1'",
            [],
        )
        .unwrap();
        assert!(search_fts(&conn, "alpha", 10).unwrap().is_empty());
        assert_eq!(search_fts(&conn, "delta", 10).unwrap().len(), 1);
        conn.execute("DELETE FROM memory_entries WHERE id = 'm1'", [])
            .unwrap();
        assert!(search_fts(&conn, "delta", 10).unwrap().is_empty());
    }
}
