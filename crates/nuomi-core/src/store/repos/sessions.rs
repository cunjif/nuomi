//! Session repository.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::Session;
use crate::store::StoreError;

pub fn insert(conn: &Connection, session: &Session) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO sessions (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            session.id,
            session.title,
            session.created_at,
            session.updated_at
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Session, StoreError> {
    conn.query_row(
        "SELECT id, title, created_at, updated_at FROM sessions WHERE id = ?1",
        params![id],
        row_to_session,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "session",
        id: id.to_string(),
    })
}

pub fn touch(conn: &Connection, id: &str, at: i64) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
        params![id, at],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "session",
            id: id.to_string(),
        });
    }
    Ok(())
}

pub fn update_title(conn: &Connection, id: &str, title: &str) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE sessions SET title = ?2 WHERE id = ?1",
        params![id, title],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "session",
            id: id.to_string(),
        });
    }
    Ok(())
}

/// Sets the session's cache-lineage scope (hermes cache-lineage root).
/// An empty scope means "unset"; consumers fall back to the session id.
pub fn set_cache_scope(conn: &Connection, id: &str, scope: &str) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE sessions SET cache_scope = ?2 WHERE id = ?1",
        params![id, scope],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "session",
            id: id.to_string(),
        });
    }
    Ok(())
}

/// Reads the cache-lineage scope. `Ok(None)` means unset (empty string),
/// which semantically falls back to the session id.
pub fn cache_scope(conn: &Connection, id: &str) -> Result<Option<String>, StoreError> {
    let scope: String = conn
        .query_row(
            "SELECT cache_scope FROM sessions WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound {
            entity: "session",
            id: id.to_string(),
        })?;
    Ok((!scope.is_empty()).then_some(scope))
}

/// Lists sessions, most recently updated first.
pub fn list(conn: &Connection, limit: u32) -> Result<Vec<Session>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, title, created_at, updated_at FROM sessions ORDER BY updated_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], row_to_session)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        ensure_cache_scope_column(&conn);
        conn
    }

    /// Migration 0007 is pending registration in `store/migrations.rs`
    /// (main session owns it), so make sure the column exists for tests.
    /// Once registered this becomes a no-op.
    fn ensure_cache_scope_column(conn: &Connection) {
        let has_column: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('sessions') WHERE name = 'cache_scope'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        if has_column == 0 {
            conn.execute(
                "ALTER TABLE sessions ADD COLUMN cache_scope TEXT NOT NULL DEFAULT ''",
                [],
            )
            .unwrap();
        }
    }

    fn session(id: &str) -> Session {
        Session {
            id: id.into(),
            title: "t".into(),
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn insert_get_roundtrip() {
        let conn = db();
        insert(&conn, &session("s1")).unwrap();
        assert_eq!(get(&conn, "s1").unwrap().id, "s1");
    }

    #[test]
    fn get_missing_is_not_found() {
        let conn = db();
        assert!(matches!(
            get(&conn, "nope"),
            Err(StoreError::NotFound {
                entity: "session",
                ..
            })
        ));
    }

    #[test]
    fn touch_updates_and_lists_recency_order() {
        let conn = db();
        insert(&conn, &session("a")).unwrap();
        insert(&conn, &session("b")).unwrap();
        touch(&conn, "a", 99).unwrap();
        let list = list(&conn, 10).unwrap();
        assert_eq!(list[0].id, "a");
        assert_eq!(list[0].updated_at, 99);
    }

    #[test]
    fn duplicate_insert_fails() {
        let conn = db();
        insert(&conn, &session("dup")).unwrap();
        assert!(insert(&conn, &session("dup")).is_err());
    }

    #[test]
    fn update_title_sets_title_and_missing_id_is_not_found() {
        let conn = db();
        insert(&conn, &session("s1")).unwrap();
        update_title(&conn, "s1", "renamed").unwrap();
        assert_eq!(get(&conn, "s1").unwrap().title, "renamed");
        assert!(matches!(
            update_title(&conn, "nope", "x"),
            Err(StoreError::NotFound {
                entity: "session",
                ..
            })
        ));
    }

    #[test]
    fn cache_scope_roundtrip_and_empty_means_unset() {
        let conn = db();
        insert(&conn, &session("s1")).unwrap();
        assert_eq!(cache_scope(&conn, "s1").unwrap(), None);
        set_cache_scope(&conn, "s1", "lineage-root").unwrap();
        assert_eq!(
            cache_scope(&conn, "s1").unwrap(),
            Some("lineage-root".into())
        );
        set_cache_scope(&conn, "s1", "").unwrap();
        assert_eq!(cache_scope(&conn, "s1").unwrap(), None);
    }

    #[test]
    fn cache_scope_missing_session_is_not_found() {
        let conn = db();
        assert!(matches!(
            cache_scope(&conn, "nope"),
            Err(StoreError::NotFound {
                entity: "session",
                ..
            })
        ));
        assert!(matches!(
            set_cache_scope(&conn, "nope", "s"),
            Err(StoreError::NotFound {
                entity: "session",
                ..
            })
        ));
    }
}
