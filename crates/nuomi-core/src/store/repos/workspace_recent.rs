//! Workspace recent-use list repository (`workspace_recent` table, migration 0024).
//!
//! Tracks recently used workspaces with LRU eviction. Pinned workspaces are
//! never evicted and are surfaced at the top of the list.

use rusqlite::{params, Connection};

use crate::store::StoreError;

/// Maximum number of recent entries retained (LRU eviction applies to
/// non-pinned entries only).
pub const RECENT_LIST_CAPACITY: usize = 20;

/// A row in `workspace_recent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentEntryRow {
    pub workspace_id: String,
    pub last_used_at: i64,
    pub is_pinned: bool,
}

const ENTITY: &str = "workspace_recent";

/// Upserts a recent entry: updates `last_used_at` (and `is_pinned` if given) if
/// the workspace already exists, otherwise inserts a new row. When the list
/// exceeds `RECENT_LIST_CAPACITY`, evicts the oldest non-pinned entry.
pub fn touch(
    conn: &Connection,
    workspace_id: &str,
    ts: i64,
    is_pinned: bool,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO workspace_recent (workspace_id, last_used_at, is_pinned)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(workspace_id) DO UPDATE SET
             last_used_at = excluded.last_used_at,
             is_pinned = excluded.is_pinned",
        params![workspace_id, ts, is_pinned as i64],
    )?;

    // Evict oldest non-pinned entry if over capacity (configurable via
    // app_settings, default RECENT_LIST_CAPACITY).
    let capacity = super::settings::get(conn, super::settings::RECENT_LIST_CAPACITY_KEY)
        .ok()
        .and_then(|v| v)
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(RECENT_LIST_CAPACITY);
    let count: i64 = conn.query_row("SELECT count(*) FROM workspace_recent", [], |r| r.get(0))?;
    if count as usize > capacity {
        conn.execute(
            "DELETE FROM workspace_recent
             WHERE workspace_id = (
                 SELECT workspace_id FROM workspace_recent
                 WHERE is_pinned = 0
                 ORDER BY last_used_at ASC
                 LIMIT 1
             )",
            [],
        )?;
    }
    Ok(())
}

/// Lists recent entries ordered by pinned-first, then `last_used_at DESC`.
/// At most `limit` entries are returned.
pub fn list(conn: &Connection, limit: usize) -> Result<Vec<RecentEntryRow>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT workspace_id, last_used_at, is_pinned
         FROM workspace_recent
         ORDER BY is_pinned DESC, last_used_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], row_to_entry)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Returns all recent entries (pinned-first, then `last_used_at DESC`).
pub fn list_all(conn: &Connection) -> Result<Vec<RecentEntryRow>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT workspace_id, last_used_at, is_pinned
         FROM workspace_recent
         ORDER BY is_pinned DESC, last_used_at DESC",
    )?;
    let rows = stmt.query_map([], row_to_entry)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Removes a recent entry. Returns `NotFound` when the id does not match.
pub fn remove(conn: &Connection, workspace_id: &str) -> Result<(), StoreError> {
    let n = conn.execute(
        "DELETE FROM workspace_recent WHERE workspace_id = ?1",
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

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecentEntryRow> {
    let is_pinned: i64 = row.get(2)?;
    Ok(RecentEntryRow {
        workspace_id: row.get(0)?,
        last_used_at: row.get(1)?,
        is_pinned: is_pinned != 0,
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

    #[test]
    fn touch_inserts_and_updates() {
        let conn = db();
        seed_workspace(&conn, "w1");
        touch(&conn, "w1", 100, false).unwrap();
        let entries = list_all(&conn).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].last_used_at, 100);
        // update timestamp
        touch(&conn, "w1", 200, false).unwrap();
        let entries = list_all(&conn).unwrap();
        assert_eq!(entries[0].last_used_at, 200);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn list_pinned_first_then_by_recency() {
        let conn = db();
        seed_workspace(&conn, "w1");
        seed_workspace(&conn, "w2");
        seed_workspace(&conn, "w3");
        touch(&conn, "w1", 300, false).unwrap();
        touch(&conn, "w2", 100, true).unwrap(); // pinned but oldest
        touch(&conn, "w3", 200, false).unwrap();
        let entries = list_all(&conn).unwrap();
        // pinned first
        assert_eq!(entries[0].workspace_id, "w2");
        // then by last_used_at DESC
        assert_eq!(entries[1].workspace_id, "w1");
        assert_eq!(entries[2].workspace_id, "w3");
    }

    #[test]
    fn list_respects_limit() {
        let conn = db();
        for i in 0..5 {
            seed_workspace(&conn, &format!("w{i}"));
            touch(&conn, &format!("w{i}"), i as i64, false).unwrap();
        }
        let entries = list(&conn, 3).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn eviction_removes_oldest_non_pinned() {
        let conn = db();
        // Fill to capacity + 1.
        for i in 0..(RECENT_LIST_CAPACITY + 1) {
            let id = format!("w{i}");
            seed_workspace(&conn, &id);
            touch(&conn, &id, i as i64, false).unwrap();
        }
        let entries = list_all(&conn).unwrap();
        assert_eq!(entries.len(), RECENT_LIST_CAPACITY);
        // oldest (w0) should have been evicted.
        assert!(entries.iter().all(|e| e.workspace_id != "w0"));
    }

    #[test]
    fn pinned_entries_not_evicted() {
        let conn = db();
        // Pin the oldest entry so it should not be evicted.
        for i in 0..(RECENT_LIST_CAPACITY + 1) {
            let id = format!("w{i}");
            seed_workspace(&conn, &id);
            touch(&conn, &id, i as i64, i == 0).unwrap();
        }
        let entries = list_all(&conn).unwrap();
        assert_eq!(entries.len(), RECENT_LIST_CAPACITY);
        // w0 (pinned) must still be present.
        assert!(entries.iter().any(|e| e.workspace_id == "w0"));
    }
}
