//! Key/value app settings (`app_settings` table, migration 0008).
//!
//! Small durable string settings that must survive restarts — currently only
//! the workspace root. Values are plain TEXT; callers own interpretation.

use rusqlite::{params, Connection, OptionalExtension};

use crate::store::StoreError;

/// Persisted sandbox root shown on first launch and restored on boot.
pub const WORKSPACE_ROOT: &str = "workspace_root";

/// Whether to restore non-pinned workspaces on startup (default: true).
pub const RESTORE_NON_PINNED_ON_STARTUP: &str = "restore_non_pinned_on_startup";

/// Max open workspaces override (default: 8). Advanced users may lower this.
pub const MAX_OPEN_WORKSPACES_KEY: &str = "max_open_workspaces";

/// Recent list capacity override (default: 20).
pub const RECENT_LIST_CAPACITY_KEY: &str = "recent_list_capacity";

/// View scope preference keys for per-surface "focused / all" toggle.
pub const VIEW_SCOPE_BOARD: &str = "view_scope.board";
pub const VIEW_SCOPE_APPROVALS: &str = "view_scope.approvals";
pub const VIEW_SCOPE_SCHEDULER: &str = "view_scope.scheduler";

/// Returns the stored value for `key`, or `None` when unset.
pub fn get(conn: &Connection, key: &str) -> Result<Option<String>, StoreError> {
    let value = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?;
    Ok(value)
}

/// Inserts or overwrites the value for `key` (UPSERT).
pub fn set(conn: &Connection, key: &str, value: &str) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO app_settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, value, crate::domain::now_ms()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;
    use rusqlite::Connection;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    #[test]
    fn get_returns_none_when_unset_then_value_after_set() {
        let conn = db();
        assert_eq!(get(&conn, WORKSPACE_ROOT).unwrap(), None);
        set(&conn, WORKSPACE_ROOT, "C:\\ws").unwrap();
        assert_eq!(get(&conn, WORKSPACE_ROOT).unwrap(), Some("C:\\ws".into()));
    }

    #[test]
    fn set_upserts_over_the_same_key() {
        let conn = db();
        set(&conn, WORKSPACE_ROOT, "first").unwrap();
        set(&conn, WORKSPACE_ROOT, "second").unwrap();
        assert_eq!(get(&conn, WORKSPACE_ROOT).unwrap(), Some("second".into()));
        // keys are independent
        set(&conn, "other", "x").unwrap();
        assert_eq!(get(&conn, WORKSPACE_ROOT).unwrap(), Some("second".into()));
    }
}
