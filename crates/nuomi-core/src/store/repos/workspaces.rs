//! Workspace registry repository (`workspaces` table, migration 0016).
//!
//! Structured CRUD for the multi-workspace registry. `root_path` carries a
//! UNIQUE constraint; `is_active` is maintained at most-one by `set_active`.

use rusqlite::{params, Connection, OptionalExtension};

use crate::store::StoreError;

/// A registered workspace entry (mirrors the `workspaces` table row).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEntry {
    pub id: String,
    pub root_path: String,
    pub color_tag: String,
    pub created_at: i64,
    pub is_active: bool,
}

const ENTITY: &str = "workspace";

/// Inserts a new registry row. Returns `AlreadyExists` when `root_path` is
/// already registered (UNIQUE constraint).
pub fn insert(conn: &Connection, entry: &WorkspaceEntry) -> Result<(), StoreError> {
    match conn.execute(
        "INSERT INTO workspaces (id, root_path, color_tag, created_at, is_active)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            entry.id,
            entry.root_path,
            entry.color_tag,
            entry.created_at,
            entry.is_active as i64,
        ],
    ) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Err(StoreError::AlreadyExists {
                entity: ENTITY,
                value: entry.root_path.clone(),
            })
        }
        Err(e) => Err(StoreError::Sqlite(e)),
    }
}

/// Lists all registered workspaces ordered by `created_at ASC`.
pub fn list(conn: &Connection) -> Result<Vec<WorkspaceEntry>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, root_path, color_tag, created_at, is_active
         FROM workspaces ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], row_to_entry)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Finds a workspace by its (normalized) root path.
pub fn find_by_path(conn: &Connection, root_path: &str) -> Result<Option<WorkspaceEntry>, StoreError> {
    conn.query_row(
        "SELECT id, root_path, color_tag, created_at, is_active
         FROM workspaces WHERE root_path = ?1",
        params![root_path],
        row_to_entry,
    )
    .optional()
    .map_err(StoreError::Sqlite)
}

/// Finds a workspace by id.
pub fn find_by_id(conn: &Connection, id: &str) -> Result<Option<WorkspaceEntry>, StoreError> {
    conn.query_row(
        "SELECT id, root_path, color_tag, created_at, is_active
         FROM workspaces WHERE id = ?1",
        params![id],
        row_to_entry,
    )
    .optional()
    .map_err(StoreError::Sqlite)
}

/// Returns the currently active workspace, if any.
pub fn find_active(conn: &Connection) -> Result<Option<WorkspaceEntry>, StoreError> {
    conn.query_row(
        "SELECT id, root_path, color_tag, created_at, is_active
         FROM workspaces WHERE is_active = 1",
        [],
        row_to_entry,
    )
    .optional()
    .map_err(StoreError::Sqlite)
}

/// Sets `id` as the sole active workspace (deactivating all others).
/// Checks existence first so a non-existent id does not clear existing
/// active flags — the clear and set are only run when the target exists.
pub fn set_active(conn: &Connection, id: &str) -> Result<(), StoreError> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM workspaces WHERE id = ?1)",
        params![id],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(StoreError::NotFound {
            entity: ENTITY,
            id: id.to_string(),
        });
    }
    conn.execute("UPDATE workspaces SET is_active = 0 WHERE is_active = 1", [])?;
    conn.execute(
        "UPDATE workspaces SET is_active = 1 WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Removes a workspace by id (DELETE). Does not touch the filesystem.
pub fn remove(conn: &Connection, id: &str) -> Result<(), StoreError> {
    let n = conn.execute("DELETE FROM workspaces WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: ENTITY,
            id: id.to_string(),
        });
    }
    Ok(())
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceEntry> {
    let is_active: i64 = row.get(4)?;
    Ok(WorkspaceEntry {
        id: row.get(0)?,
        root_path: row.get(1)?,
        color_tag: row.get(2)?,
        created_at: row.get(3)?,
        is_active: is_active != 0,
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

    fn entry(id: &str, path: &str) -> WorkspaceEntry {
        WorkspaceEntry {
            id: id.into(),
            root_path: path.into(),
            color_tag: "paper-yellow".into(),
            created_at: 1,
            is_active: false,
        }
    }

    #[test]
    fn insert_list_roundtrip() {
        let conn = db();
        insert(&conn, &entry("w1", "C:\\ws1")).unwrap();
        insert(&conn, &entry("w2", "C:\\ws2")).unwrap();
        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, "w1");
        assert_eq!(all[1].id, "w2");
    }

    #[test]
    fn duplicate_root_path_returns_already_exists() {
        let conn = db();
        insert(&conn, &entry("w1", "C:\\ws")).unwrap();
        let err = insert(&conn, &entry("w2", "C:\\ws")).unwrap_err();
        assert!(matches!(
            err,
            StoreError::AlreadyExists {
                entity: "workspace",
                ..
            }
        ));
    }

    #[test]
    fn find_by_path_and_id() {
        let conn = db();
        insert(&conn, &entry("w1", "C:\\ws")).unwrap();
        assert_eq!(find_by_path(&conn, "C:\\ws").unwrap().unwrap().id, "w1");
        assert_eq!(find_by_id(&conn, "w1").unwrap().unwrap().id, "w1");
        assert!(find_by_path(&conn, "none").unwrap().is_none());
        assert!(find_by_id(&conn, "none").unwrap().is_none());
    }

    #[test]
    fn find_active_returns_none_then_active() {
        let conn = db();
        assert!(find_active(&conn).unwrap().is_none());
        let mut e = entry("w1", "C:\\ws");
        e.is_active = true;
        insert(&conn, &e).unwrap();
        assert_eq!(find_active(&conn).unwrap().unwrap().id, "w1");
    }

    #[test]
    fn set_active_ensures_uniqueness() {
        let conn = db();
        insert(&conn, &entry("w1", "C:\\ws1")).unwrap();
        insert(&conn, &entry("w2", "C:\\ws2")).unwrap();
        set_active(&conn, "w1").unwrap();
        assert_eq!(find_active(&conn).unwrap().unwrap().id, "w1");
        set_active(&conn, "w2").unwrap();
        let active = find_active(&conn).unwrap().unwrap();
        assert_eq!(active.id, "w2");
        // 至多一条 is_active=1
        let count: i64 = conn
            .query_row("SELECT count(*) FROM workspaces WHERE is_active=1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn set_active_missing_id_is_not_found() {
        let conn = db();
        assert!(matches!(
            set_active(&conn, "nope"),
            Err(StoreError::NotFound {
                entity: "workspace",
                ..
            })
        ));
    }

    #[test]
    fn remove_deletes_and_missing_is_not_found() {
        let conn = db();
        insert(&conn, &entry("w1", "C:\\ws")).unwrap();
        remove(&conn, "w1").unwrap();
        assert!(list(&conn).unwrap().is_empty());
        assert!(matches!(
            remove(&conn, "w1"),
            Err(StoreError::NotFound {
                entity: "workspace",
                ..
            })
        ));
    }
}
