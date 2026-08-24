//! Prompt version repository: candidate → active → retired state machine.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{PromptStatus, PromptVersion};
use crate::store::StoreError;

/// Inserts a candidate version. `version` must not collide within the plugin.
pub fn insert_candidate(conn: &Connection, v: &PromptVersion) -> Result<(), StoreError> {
    if v.status != PromptStatus::Candidate {
        return Err(StoreError::NotFound {
            entity: "prompt_version",
            id: format!("{} must be inserted as candidate", v.id),
        });
    }
    conn.execute(
        "INSERT INTO prompt_versions
         (id, plugin, version, status, content, diff_text, parent_version, activated_at, created_at)
         VALUES (?1, ?2, ?3, 'candidate', ?4, ?5, ?6, NULL, ?7)",
        params![
            v.id,
            v.plugin,
            v.version,
            v.content,
            v.diff_text,
            v.parent_version,
            v.created_at
        ],
    )?;
    Ok(())
}

/// Activates a candidate: retires any currently active version atomically.
pub fn activate(conn: &Connection, plugin: &str, version: i64, at: i64) -> Result<(), StoreError> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE prompt_versions SET status = 'retired' WHERE plugin = ?1 AND status = 'active'",
        params![plugin],
    )?;
    let n = tx.execute(
        "UPDATE prompt_versions SET status = 'active', activated_at = ?3
         WHERE plugin = ?1 AND version = ?2 AND status = 'candidate'",
        params![plugin, version, at],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "prompt_version",
            id: format!("{plugin}@v{version} is not an activatable candidate"),
        });
    }
    tx.commit()?;
    Ok(())
}

pub fn get_active(conn: &Connection, plugin: &str) -> Result<PromptVersion, StoreError> {
    conn.query_row(
        "SELECT id, plugin, version, status, content, diff_text, parent_version, activated_at, created_at
         FROM prompt_versions WHERE plugin = ?1 AND status = 'active'",
        params![plugin],
        row_prompt,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound { entity: "prompt_version", id: format!("{plugin} has no active version") })
}

pub fn list_by_plugin(conn: &Connection, plugin: &str) -> Result<Vec<PromptVersion>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, plugin, version, status, content, diff_text, parent_version, activated_at, created_at
         FROM prompt_versions WHERE plugin = ?1 ORDER BY version DESC",
    )?;
    let rows = stmt.query_map(params![plugin], row_prompt)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn row_prompt(row: &rusqlite::Row<'_>) -> rusqlite::Result<PromptVersion> {
    let status: String = row.get(3)?;
    let status = match status.as_str() {
        "candidate" => PromptStatus::Candidate,
        "active" => PromptStatus::Active,
        "retired" => PromptStatus::Retired,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                format!("unknown status {other}").into(),
            ))
        }
    };
    Ok(PromptVersion {
        id: row.get(0)?,
        plugin: row.get(1)?,
        version: row.get(2)?,
        status,
        content: row.get(4)?,
        diff_text: row.get(5)?,
        parent_version: row.get(6)?,
        activated_at: row.get(7)?,
        created_at: row.get(8)?,
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

    fn candidate(version: i64, content: &str) -> PromptVersion {
        PromptVersion {
            id: format!("pv{version}"),
            plugin: "system_prompt".into(),
            version,
            status: PromptStatus::Candidate,
            content: content.into(),
            diff_text: Some("diff".into()),
            parent_version: Some(version - 1),
            activated_at: None,
            created_at: 1,
        }
    }

    #[test]
    fn candidate_then_activate_then_supersede() {
        let conn = db();
        insert_candidate(&conn, &candidate(1, "v1")).unwrap();
        activate(&conn, "system_prompt", 1, 10).unwrap();
        let active = get_active(&conn, "system_prompt").unwrap();
        assert_eq!((active.version, active.status), (1, PromptStatus::Active));
        assert_eq!(active.activated_at, Some(10));

        // v2 supersedes v1; v1 retires.
        insert_candidate(&conn, &candidate(2, "v2")).unwrap();
        activate(&conn, "system_prompt", 2, 20).unwrap();
        assert_eq!(get_active(&conn, "system_prompt").unwrap().version, 2);
        let all = list_by_plugin(&conn, "system_prompt").unwrap();
        let retired = all.iter().find(|v| v.version == 1).unwrap();
        assert_eq!(retired.status, PromptStatus::Retired);
    }

    #[test]
    fn activating_nonexistent_or_non_candidate_fails() {
        let conn = db();
        assert!(activate(&conn, "system_prompt", 99, 1).is_err());
        insert_candidate(&conn, &candidate(1, "v1")).unwrap();
        activate(&conn, "system_prompt", 1, 10).unwrap();
        // already active, no longer a candidate
        assert!(activate(&conn, "system_prompt", 1, 30).is_err());
    }

    #[test]
    fn duplicate_plugin_version_rejected() {
        let conn = db();
        insert_candidate(&conn, &candidate(1, "a")).unwrap();
        let mut dup = candidate(1, "b");
        dup.id = "other".into();
        assert!(insert_candidate(&conn, &dup).is_err());
    }

    #[test]
    fn no_active_version_is_not_found() {
        let conn = db();
        assert!(get_active(&conn, "system_prompt").is_err());
    }
}
