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

// -----------------------------------------------------------------------
// Explicit-exclusion marker (migration 0042).
//
// `workspace_recent` is a recency *cache*, not a deletion-state store. When
// the user removes an entry via `remove_recent` / `clear_recent`, the
// underlying `workspaces` entity is kept (the workspace is still registered).
// On startup, `restore_from_snapshot` walks `workspaces` and calls `touch` for
// each, which re-inserts the removed row — the entry "resurrects".
//
// `workspace_recent_excluded` persists the "user explicitly removed this from
// the recent list" signal. `touch_if_not_excluded` (restore path) skips
// excluded workspaces; `touch_clearing_exclusion` (user-initiated open/focus)
// drops the marker and re-joins the list ("reuse overrides prior explicit
// removal").
// -----------------------------------------------------------------------

/// Returns true when `workspace_id` is explicitly excluded from the recent
/// list (user previously removed it).
pub fn is_excluded(conn: &Connection, workspace_id: &str) -> Result<bool, StoreError> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM workspace_recent_excluded WHERE workspace_id = ?1",
        params![workspace_id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Marks `workspace_id` as explicitly excluded from the recent list.
/// Idempotent (updates `excluded_at` on conflict). No-op when the workspace
/// entity does not exist — the exclusion marker only matters for entries that
/// `restore_from_snapshot` (which walks `workspaces`) could resurrect, and a
/// missing entity is never walked. The `WHERE EXISTS` guard also satisfies
/// the `REFERENCES workspaces(id)` foreign key without requiring callers to
/// pre-check existence.
pub fn add_exclusion(conn: &Connection, workspace_id: &str, ts: i64) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO workspace_recent_excluded (workspace_id, excluded_at)
         SELECT ?1, ?2
         WHERE EXISTS (SELECT 1 FROM workspaces WHERE id = ?1)
         ON CONFLICT(workspace_id) DO UPDATE SET
             excluded_at = excluded.excluded_at",
        params![workspace_id, ts],
    )?;
    Ok(())
}

/// Marks every workspace currently in the recent list as excluded. Used by
/// `clear_recent` so that the startup restore pass does not resurrect the
/// cleared entries. Must be called *before* `clear_all` (it reads
/// `workspace_recent`). Idempotent over prior per-id exclusions.
pub fn add_exclusion_for_all_recent(conn: &Connection, ts: i64) -> Result<(), StoreError> {
    conn.execute(
        "INSERT OR IGNORE INTO workspace_recent_excluded (workspace_id, excluded_at)
         SELECT workspace_id, ?1 FROM workspace_recent",
        params![ts],
    )?;
    Ok(())
}

/// Clears the explicit-exclusion marker for `workspace_id`. Idempotent.
pub fn remove_exclusion(conn: &Connection, workspace_id: &str) -> Result<(), StoreError> {
    conn.execute(
        "DELETE FROM workspace_recent_excluded WHERE workspace_id = ?1",
        params![workspace_id],
    )?;
    Ok(())
}

/// Touches the recent list but skips when the workspace is explicitly
/// excluded. Used by the startup restore path so that user-removed entries
/// do not reappear after restart.
pub fn touch_if_not_excluded(
    conn: &Connection,
    workspace_id: &str,
    ts: i64,
    is_pinned: bool,
) -> Result<(), StoreError> {
    if is_excluded(conn, workspace_id)? {
        return Ok(());
    }
    touch(conn, workspace_id, ts, is_pinned)
}

/// Touches the recent list and clears any explicit-exclusion marker first.
/// Used by user-initiated open/focus so that reopening a previously-removed
/// workspace brings it back into the recent list.
pub fn touch_clearing_exclusion(
    conn: &Connection,
    workspace_id: &str,
    ts: i64,
    is_pinned: bool,
) -> Result<(), StoreError> {
    remove_exclusion(conn, workspace_id)?;
    touch(conn, workspace_id, ts, is_pinned)
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

/// Idempotently removes a recent entry. Always returns `Ok(())` regardless of
/// whether the row existed. Satisfies the "target already absent" idempotent
/// success semantic (spec 4.2-1) without altering the existing `remove`
/// signature that some callers may rely on for `NotFound`.
pub fn remove_idempotent(conn: &Connection, workspace_id: &str) -> Result<(), StoreError> {
    conn.execute(
        "DELETE FROM workspace_recent WHERE workspace_id = ?1",
        params![workspace_id],
    )?;
    Ok(())
}

/// Clears the entire `workspace_recent` table. Idempotent: clearing an empty
/// table still returns `Ok(())`.
pub fn clear_all(conn: &Connection) -> Result<(), StoreError> {
    conn.execute("DELETE FROM workspace_recent", [])?;
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

    #[test]
    fn remove_idempotent_deletes_existing_row() {
        let conn = db();
        seed_workspace(&conn, "w1");
        touch(&conn, "w1", 100, false).unwrap();
        assert_eq!(list_all(&conn).unwrap().len(), 1);
        remove_idempotent(&conn, "w1").unwrap();
        assert!(list_all(&conn).unwrap().is_empty());
    }

    #[test]
    fn remove_idempotent_succeeds_when_row_absent() {
        let conn = db();
        seed_workspace(&conn, "w1");
        touch(&conn, "w1", 100, false).unwrap();
        // Remove once.
        remove_idempotent(&conn, "w1").unwrap();
        // Remove again — must not error (idempotent).
        remove_idempotent(&conn, "w1").unwrap();
        // Non-existent id also succeeds.
        remove_idempotent(&conn, "never-existed").unwrap();
        assert!(list_all(&conn).unwrap().is_empty());
    }

    #[test]
    fn clear_all_empties_table() {
        let conn = db();
        seed_workspace(&conn, "w1");
        seed_workspace(&conn, "w2");
        seed_workspace(&conn, "w3");
        touch(&conn, "w1", 100, false).unwrap();
        touch(&conn, "w2", 200, true).unwrap();
        touch(&conn, "w3", 300, false).unwrap();
        assert_eq!(list_all(&conn).unwrap().len(), 3);
        clear_all(&conn).unwrap();
        assert_eq!(list_all(&conn).unwrap().len(), 0);
    }

    #[test]
    fn clear_all_on_empty_table_succeeds() {
        let conn = db();
        clear_all(&conn).unwrap();
        assert!(list_all(&conn).unwrap().is_empty());
        // Idempotent: clear again.
        clear_all(&conn).unwrap();
    }

    // ---- explicit-exclusion marker (migration 0042) ----

    #[test]
    fn is_excluded_false_by_default() {
        let conn = db();
        seed_workspace(&conn, "w1");
        assert!(!is_excluded(&conn, "w1").unwrap());
    }

    #[test]
    fn add_exclusion_marks_and_is_idempotent() {
        let conn = db();
        seed_workspace(&conn, "w1");
        add_exclusion(&conn, "w1", 100).unwrap();
        assert!(is_excluded(&conn, "w1").unwrap());
        // Re-mark with a later timestamp — still excluded, no error.
        add_exclusion(&conn, "w1", 200).unwrap();
        assert!(is_excluded(&conn, "w1").unwrap());
    }

    #[test]
    fn remove_exclusion_clears_marker_and_is_idempotent() {
        let conn = db();
        seed_workspace(&conn, "w1");
        add_exclusion(&conn, "w1", 100).unwrap();
        remove_exclusion(&conn, "w1").unwrap();
        assert!(!is_excluded(&conn, "w1").unwrap());
        // Removing an absent marker is a no-op.
        remove_exclusion(&conn, "w1").unwrap();
        assert!(!is_excluded(&conn, "w1").unwrap());
    }

    #[test]
    fn touch_if_not_excluded_skips_excluded_workspace() {
        let conn = db();
        seed_workspace(&conn, "w1");
        touch(&conn, "w1", 100, false).unwrap();
        // User removes from recent → exclusion marker set, cache row deleted.
        remove_idempotent(&conn, "w1").unwrap();
        add_exclusion(&conn, "w1", 200).unwrap();
        assert!(list_all(&conn).unwrap().is_empty());
        // Restore-path touch must NOT resurrect the entry.
        touch_if_not_excluded(&conn, "w1", 300, false).unwrap();
        assert!(list_all(&conn).unwrap().is_empty());
    }

    #[test]
    fn touch_if_not_excluded_touches_when_not_excluded() {
        let conn = db();
        seed_workspace(&conn, "w1");
        // No exclusion marker → behaves like plain touch.
        touch_if_not_excluded(&conn, "w1", 100, false).unwrap();
        let entries = list_all(&conn).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].workspace_id, "w1");
    }

    #[test]
    fn touch_clearing_exclusion_revives_excluded_workspace() {
        let conn = db();
        seed_workspace(&conn, "w1");
        touch(&conn, "w1", 100, false).unwrap();
        remove_idempotent(&conn, "w1").unwrap();
        add_exclusion(&conn, "w1", 200).unwrap();
        assert!(is_excluded(&conn, "w1").unwrap());
        // User-initiated open/focus → marker cleared, entry rejoins recent.
        touch_clearing_exclusion(&conn, "w1", 300, false).unwrap();
        assert!(!is_excluded(&conn, "w1").unwrap());
        let entries = list_all(&conn).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].workspace_id, "w1");
        assert_eq!(entries[0].last_used_at, 300);
    }

    #[test]
    fn add_exclusion_for_all_recent_marks_every_entry() {
        let conn = db();
        seed_workspace(&conn, "w1");
        seed_workspace(&conn, "w2");
        seed_workspace(&conn, "w3");
        touch(&conn, "w1", 100, false).unwrap();
        touch(&conn, "w2", 200, true).unwrap();
        touch(&conn, "w3", 300, false).unwrap();
        // Mark all recent entries as excluded (clear_recent pre-step).
        add_exclusion_for_all_recent(&conn, 400).unwrap();
        clear_all(&conn).unwrap();
        // Every previously-recent workspace is now excluded.
        assert!(is_excluded(&conn, "w1").unwrap());
        assert!(is_excluded(&conn, "w2").unwrap());
        assert!(is_excluded(&conn, "w3").unwrap());
        // Restore-path touch skips all of them.
        touch_if_not_excluded(&conn, "w1", 500, false).unwrap();
        touch_if_not_excluded(&conn, "w2", 500, true).unwrap();
        touch_if_not_excluded(&conn, "w3", 500, false).unwrap();
        assert!(list_all(&conn).unwrap().is_empty());
    }

    #[test]
    fn add_exclusion_for_all_recent_is_idempotent_with_prior_markers() {
        let conn = db();
        seed_workspace(&conn, "w1");
        seed_workspace(&conn, "w2");
        touch(&conn, "w1", 100, false).unwrap();
        touch(&conn, "w2", 200, false).unwrap();
        // w1 already excluded from a prior single remove.
        add_exclusion(&conn, "w1", 150).unwrap();
        // Bulk-mark must not error on the pre-existing w1 marker.
        add_exclusion_for_all_recent(&conn, 400).unwrap();
        assert!(is_excluded(&conn, "w1").unwrap());
        assert!(is_excluded(&conn, "w2").unwrap());
    }
}
