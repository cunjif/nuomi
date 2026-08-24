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
        conn
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
}
