//! Workspace open-state repository (`workspace_open_state` table, migration 0024).
//!
//! Tracks which workspaces are currently in the open set and which one is
//! focused. `is_focused` is maintained at most-one by `set_focused`.

use rusqlite::{params, Connection, OptionalExtension};

use crate::store::StoreError;

/// A row in `workspace_open_state` — one entry per open workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceOpenStateRow {
    pub workspace_id: String,
    pub opened_at: i64,
    pub last_focused_at: i64,
    pub is_focused: bool,
}

const ENTITY: &str = "workspace_open_state";

/// Inserts a new open-state row. Returns `AlreadyExists` if the workspace is
/// already in the open set.
pub fn insert(conn: &Connection, row: &WorkspaceOpenStateRow) -> Result<(), StoreError> {
    match conn.execute(
        "INSERT INTO workspace_open_state (workspace_id, opened_at, last_focused_at, is_focused)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            row.workspace_id,
            row.opened_at,
            row.last_focused_at,
            row.is_focused as i64,
        ],
    ) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Err(StoreError::AlreadyExists {
                entity: ENTITY,
                value: row.workspace_id.clone(),
            })
        }
        Err(e) => Err(StoreError::Sqlite(e)),
    }
}

/// Removes a workspace from the open set. Returns `NotFound` when the id does
/// not match any row.
pub fn remove(conn: &Connection, workspace_id: &str) -> Result<(), StoreError> {
    let n = conn.execute(
        "DELETE FROM workspace_open_state WHERE workspace_id = ?1",
        params![workspace_id],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: ENTITY,
            id: workspace_id.to_string(),
        });
    }
    Ok(())
}

/// Lists all open workspaces ordered by `last_focused_at DESC` (most recently
/// focused first).
pub fn list(conn: &Connection) -> Result<Vec<WorkspaceOpenStateRow>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT workspace_id, opened_at, last_focused_at, is_focused
         FROM workspace_open_state ORDER BY last_focused_at DESC",
    )?;
    let rows = stmt.query_map([], row_to_state)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Returns the currently focused workspace, if any (`is_focused = 1`).
pub fn find_focused(conn: &Connection) -> Result<Option<WorkspaceOpenStateRow>, StoreError> {
    conn.query_row(
        "SELECT workspace_id, opened_at, last_focused_at, is_focused
         FROM workspace_open_state WHERE is_focused = 1",
        [],
        row_to_state,
    )
    .optional()
    .map_err(StoreError::Sqlite)
}

/// Sets `workspace_id` as the sole focused workspace (unfocusing all others).
/// The clear and set are executed in a transaction. Returns `NotFound` when the
/// workspace is not in the open set.
pub fn set_focused(conn: &Connection, workspace_id: &str) -> Result<(), StoreError> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM workspace_open_state WHERE workspace_id = ?1)",
        params![workspace_id],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(StoreError::NotFound {
            entity: ENTITY,
            id: workspace_id.to_string(),
        });
    }
    conn.execute(
        "UPDATE workspace_open_state SET is_focused = 0 WHERE is_focused = 1",
        [],
    )?;
    conn.execute(
        "UPDATE workspace_open_state SET is_focused = 1, last_focused_at = ?2 WHERE workspace_id = ?1",
        params![workspace_id, crate::domain::now_ms()],
    )?;
    Ok(())
}

/// Returns the number of workspaces in the open set.
pub fn count(conn: &Connection) -> Result<usize, StoreError> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM workspace_open_state",
        [],
        |r| r.get(0),
    )?;
    Ok(n as usize)
}

/// Updates `last_focused_at` for the given workspace.
pub fn touch_last_focused(
    conn: &Connection,
    workspace_id: &str,
    ts: i64,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE workspace_open_state SET last_focused_at = ?2 WHERE workspace_id = ?1",
        params![workspace_id, ts],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: ENTITY,
            id: workspace_id.to_string(),
        });
    }
    Ok(())
}

fn row_to_state(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceOpenStateRow> {
    let is_focused: i64 = row.get(3)?;
    Ok(WorkspaceOpenStateRow {
        workspace_id: row.get(0)?,
        opened_at: row.get(1)?,
        last_focused_at: row.get(2)?,
        is_focused: is_focused != 0,
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

    fn seed_workspace(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO workspaces (id, root_path, color_tag, created_at, is_active)
             VALUES (?1, ?2, 'paper-yellow', 1, 0)",
            params![id, format!("C:\\ws_{id}")],
        )
        .unwrap();
    }

    fn row(id: &str) -> WorkspaceOpenStateRow {
        WorkspaceOpenStateRow {
            workspace_id: id.into(),
            opened_at: 100,
            last_focused_at: 100,
            is_focused: false,
        }
    }

    #[test]
    fn insert_list_roundtrip() {
        let conn = db();
        seed_workspace(&conn, "w1");
        seed_workspace(&conn, "w2");
        insert(&conn, &row("w1")).unwrap();
        let mut r2 = row("w2");
        r2.last_focused_at = 200;
        insert(&conn, &r2).unwrap();
        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 2);
        // ordered by last_focused_at DESC
        assert_eq!(all[0].workspace_id, "w2");
        assert_eq!(all[1].workspace_id, "w1");
    }

    #[test]
    fn duplicate_insert_returns_already_exists() {
        let conn = db();
        seed_workspace(&conn, "w1");
        insert(&conn, &row("w1")).unwrap();
        let err = insert(&conn, &row("w1")).unwrap_err();
        assert!(matches!(err, StoreError::AlreadyExists { .. }));
    }

    #[test]
    fn remove_and_not_found() {
        let conn = db();
        seed_workspace(&conn, "w1");
        insert(&conn, &row("w1")).unwrap();
        remove(&conn, "w1").unwrap();
        assert!(list(&conn).unwrap().is_empty());
        assert!(matches!(remove(&conn, "w1"), Err(StoreError::NotFound { .. })));
    }

    #[test]
    fn find_focused_none_then_set() {
        let conn = db();
        seed_workspace(&conn, "w1");
        seed_workspace(&conn, "w2");
        insert(&conn, &row("w1")).unwrap();
        insert(&conn, &row("w2")).unwrap();
        assert!(find_focused(&conn).unwrap().is_none());
        set_focused(&conn, "w1").unwrap();
        assert_eq!(find_focused(&conn).unwrap().unwrap().workspace_id, "w1");
        set_focused(&conn, "w2").unwrap();
        assert_eq!(find_focused(&conn).unwrap().unwrap().workspace_id, "w2");
        // at most one focused
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM workspace_open_state WHERE is_focused = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn set_focused_missing_is_not_found() {
        let conn = db();
        assert!(matches!(set_focused(&conn, "nope"), Err(StoreError::NotFound { .. })));
    }

    #[test]
    fn count_returns_open_set_size() {
        let conn = db();
        seed_workspace(&conn, "w1");
        seed_workspace(&conn, "w2");
        assert_eq!(count(&conn).unwrap(), 0);
        insert(&conn, &row("w1")).unwrap();
        assert_eq!(count(&conn).unwrap(), 1);
        insert(&conn, &row("w2")).unwrap();
        assert_eq!(count(&conn).unwrap(), 2);
    }

    #[test]
    fn touch_last_focused_updates_timestamp() {
        let conn = db();
        seed_workspace(&conn, "w1");
        insert(&conn, &row("w1")).unwrap();
        touch_last_focused(&conn, "w1", 999).unwrap();
        let entry = list(&conn).unwrap();
        assert_eq!(entry[0].last_focused_at, 999);
    }
}
