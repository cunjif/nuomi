//! Legacy single-workspace migration orchestration.
//!
//! On boot, after migrations 0016/0017 run, this reads the legacy
//! `app_settings.workspace_root` key and, if a placeholder row exists in the
//! `workspaces` table (inserted by 0016), corrects it with a real uuid-v7 id,
//! canonicalized path and hash-derived color. Sessions carrying the
//! `__migrated__` placeholder workspace_id are then reparented to the real id.
//! Finally the active workspace's codebase-memory products are migrated.
//!
//! After migration 0036 (work directories, ADR-0021) the same boot pass also
//! corrects placeholder `work_directories` rows: the SQL migration inserts
//! them with a temporary hex id and `display_name = abs_path`; this module
//! replaces ids with uuid-v7 values and derives precise display names
//! (cross-platform separators, trailing slashes, same-workspace collision
//! disambiguation). Rows whose path cannot be parsed are skipped with the
//! original data preserved and a `workspace.migration_report` event appended.

use std::collections::BTreeMap;
use std::path::PathBuf;

use rusqlite::Connection;
use thiserror::Error;
use uuid::Uuid;

use super::codebase_memory_migrator;
use super::workspace_palette;
use crate::store::repos::events;
use crate::store::repos::settings;
use crate::store::repos::workspaces as repo;
use crate::store::repos::workspaces::WorkspaceEntry;
use crate::store::StoreError;

const MIGRATED_PLACEHOLDER: &str = "__migrated__";

#[derive(Debug, Error)]
pub enum MigrationOrchestrationError {
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<codebase_memory_migrator::MigrationError> for MigrationOrchestrationError {
    fn from(e: codebase_memory_migrator::MigrationError) -> Self {
        match e {
            codebase_memory_migrator::MigrationError::Io(io) => MigrationOrchestrationError::Io(io),
        }
    }
}

/// Outcome of a `run_if_needed` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// A legacy workspace was migrated into the registry.
    Migrated { workspace_id: String },
    /// No legacy root found and registry non-empty — normal boot.
    Skipped,
    /// No legacy root and registry empty — first launch, UI should show setup.
    NeedsSetup,
}

/// Runs the legacy single-workspace migration if needed. Called on boot after
/// migrations 0016/0017 have been applied.
pub fn run_if_needed(conn: &Connection) -> Result<MigrationOutcome, MigrationOrchestrationError> {
    // 0036 work-directory placeholder correction (idempotent — a second boot
    // finds no placeholder rows). Runs before the legacy workspace pass so
    // directory rows exist before `correct_placeholder` may rewrite the
    // owning workspace id.
    migrate_work_directories(conn)?;

    let legacy_root = settings::get(conn, settings::WORKSPACE_ROOT)?;
    let existing = repo::list(conn)?;

    match (legacy_root, existing.is_empty()) {
        (None, true) => Ok(MigrationOutcome::NeedsSetup),
        (None, false) => {
            ensure_active_in_open_set(conn)?;
            reclaim_migrated_tasks_runs(conn)?;
            Ok(MigrationOutcome::Skipped)
        }
        (Some(root_path), _) => {
            // Legacy root exists — check if the registry has a placeholder row
            // (id not in uuid-v7 format) or needs a fresh entry.
            let migrated_id = if let Some(placeholder) = find_placeholder(&existing) {
                // Correct the placeholder in place.
                let new_id = Uuid::now_v7().to_string();
                let color = color_for_path(&root_path);
                correct_placeholder(conn, &placeholder.id, &new_id, &root_path, &color)?;
                new_id
            } else if let Some(real) = existing.iter().find(|e| is_valid_uuid(&e.id)) {
                // Already migrated (real uuid row present).
                real.id.clone()
            } else {
                // No placeholder and no real row — insert a fresh entry.
                let entry = WorkspaceEntry {
                    id: Uuid::now_v7().to_string(),
                    root_path: root_path.clone(),
                    color_tag: color_for_path(&root_path),
                    created_at: crate::domain::now_ms(),
                    is_active: existing.is_empty(),
                    is_pinned: false,
                };
                repo::insert(conn, &entry)?;
                if entry.is_active {
                    repo::set_active(conn, &entry.id)?;
                }
                entry.id
            };

            // Reparent sessions carrying the placeholder workspace_id.
            reparent_migrated_sessions(conn, &migrated_id)?;

            // Reparent tasks/runs carrying the placeholder workspace_id.
            reparent_migrated_tasks_runs(conn, &migrated_id)?;

            // Ensure the active workspace is present in workspace_open_state.
            ensure_active_in_open_set(conn)?;

            // Migrate codebase-memory products for the active workspace.
            if let Some(active) = repo::find_active(conn)? {
                let root = PathBuf::from(&active.root_path);
                if root.is_dir() {
                    let _ = codebase_memory_migrator::migrate(&root)?;
                }
            }

            Ok(MigrationOutcome::Migrated {
                workspace_id: migrated_id,
            })
        }
    }
}

/// Returns the first entry whose id is not a valid uuid (placeholder from 0016).
fn find_placeholder(entries: &[WorkspaceEntry]) -> Option<&WorkspaceEntry> {
    entries.iter().find(|e| !is_valid_uuid(&e.id))
}

/// Returns true when `s` parses as a uuid (any version).
fn is_valid_uuid(s: &str) -> bool {
    Uuid::parse_str(s).is_ok()
}

fn color_for_path(path: &str) -> String {
    workspace_palette::color_for(path).0.to_string()
}

fn correct_placeholder(
    conn: &Connection,
    old_id: &str,
    new_id: &str,
    root_path: &str,
    color: &str,
) -> Result<(), MigrationOrchestrationError> {
    // Rewriting the workspace id mutates a parent key while FK children may
    // still reference the old id — immediate FK enforcement rejects either
    // statement order, and `root_path` is UNIQUE so the row cannot be
    // re-inserted under the new id first. A transaction with deferred FK
    // checks runs every statement and validates referential integrity once,
    // at commit.
    let tx = conn.unchecked_transaction().map_err(StoreError::Sqlite)?;
    tx.execute_batch("PRAGMA defer_foreign_keys = ON")
        .map_err(StoreError::Sqlite)?;
    tx.execute(
        "UPDATE workspaces SET id = ?1, root_path = ?2, color_tag = ?3 WHERE id = ?4",
        rusqlite::params![new_id, root_path, color, old_id],
    )
    .map_err(StoreError::Sqlite)?;
    // Keep FK children owned by the placeholder id consistent with the
    // rewritten id: the 0024 open-set/recent rows (a cross-version upgrade
    // backfills them under the placeholder before this Rust pass runs) and
    // the 0036 backfilled work-directory row.
    for table in [
        "workspace_open_state",
        "workspace_recent",
        "work_directories",
    ] {
        tx.execute(
            &format!("UPDATE {table} SET workspace_id = ?1 WHERE workspace_id = ?2"),
            rusqlite::params![new_id, old_id],
        )
        .map_err(StoreError::Sqlite)?;
    }
    tx.commit().map_err(StoreError::Sqlite)?;
    Ok(())
}

/// Outcome of the 0036 work-directory placeholder correction pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WorkDirectoryCorrection {
    /// Placeholder rows corrected with a real uuid id and precise display name.
    pub corrected: usize,
    /// Rows skipped because the path could not be parsed. Original data is
    /// preserved and a `workspace.migration_report` event was appended.
    pub skipped: usize,
}

/// A `work_directories` row as inserted by migration 0036.
#[derive(Debug, Clone)]
struct DirectoryRow {
    id: String,
    workspace_id: String,
    abs_path: String,
}

/// Corrects placeholder `work_directories` rows left by migration 0036:
/// replaces the temporary hex id with a uuid-v7, and replaces the
/// `display_name = abs_path` placeholder with the precise directory name
/// (cross-platform separators, trailing slashes, same-workspace collision
/// disambiguation). Idempotent — rows whose id already parses as a uuid are
/// left untouched, so a second boot is a no-op.
pub(crate) fn migrate_work_directories(
    conn: &Connection,
) -> Result<WorkDirectoryCorrection, MigrationOrchestrationError> {
    let mut stmt = conn
        .prepare(
            "SELECT id, workspace_id, abs_path FROM work_directories ORDER BY workspace_id, added_at",
        )
        .map_err(StoreError::Sqlite)?;
    let rows = stmt
        .query_map([], |row| {
            Ok(DirectoryRow {
                id: row.get(0)?,
                workspace_id: row.get(1)?,
                abs_path: row.get(2)?,
            })
        })
        .map_err(StoreError::Sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StoreError::Sqlite)?;
    drop(stmt);

    // Placeholder rows are those whose id is not a valid uuid (0016-style
    // SQL placeholder). Rows inserted later by the repository always carry
    // real uuid-v7 ids and precise display names.
    let placeholders: Vec<DirectoryRow> =
        rows.into_iter().filter(|r| !is_valid_uuid(&r.id)).collect();
    if placeholders.is_empty() {
        return Ok(WorkDirectoryCorrection::default());
    }

    let mut report = WorkDirectoryCorrection::default();

    // Group by workspace so collision disambiguation stays per-workspace.
    let mut by_workspace: BTreeMap<String, Vec<DirectoryRow>> = BTreeMap::new();
    for row in placeholders {
        by_workspace
            .entry(row.workspace_id.clone())
            .or_default()
            .push(row);
    }

    let now = crate::domain::now_ms();
    for (workspace_id, group) in by_workspace {
        // Extract (display_name, prefix) per row; skip rows whose path cannot
        // be parsed, preserving the original row and appending an audit event.
        let mut valid: Vec<(DirectoryRow, String, String)> = Vec::new();
        for row in group {
            match split_display_name(&row.abs_path) {
                Some((display, prefix)) => valid.push((row, display, prefix)),
                None => {
                    report.skipped += 1;
                    let payload = serde_json::json!({
                        "migration": "0036_work_directories",
                        "directory_id": row.id,
                        "abs_path": row.abs_path,
                        "issue": "path_invalid",
                    });
                    events::append(
                        conn,
                        "workspace",
                        &workspace_id,
                        "migration_report",
                        &payload,
                        now,
                    )?;
                }
            }
        }

        // Disambiguate same-workspace display-name collisions.
        let display_names = disambiguate(&valid);

        for ((row, _, _), display) in valid.iter().zip(display_names) {
            let new_id = Uuid::now_v7().to_string();
            conn.execute(
                "UPDATE work_directories SET id = ?1, display_name = ?2 WHERE id = ?3",
                rusqlite::params![new_id, display, row.id],
            )
            .map_err(StoreError::Sqlite)?;
            report.corrected += 1;
        }
    }

    Ok(report)
}

/// Splits an absolute path into `(display_name, prefix)` where `display_name`
/// is the final path segment (cross-platform separators, trailing slashes
/// trimmed) and `prefix` is everything before it. Returns `None` when the
/// path has no usable final segment (empty or separators only).
fn split_display_name(abs_path: &str) -> Option<(String, String)> {
    let trimmed = abs_path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.rfind(['/', '\\']) {
        Some(pos) => Some((trimmed[pos + 1..].to_string(), trimmed[..pos].to_string())),
        None => Some((trimmed.to_string(), String::new())),
    }
}

/// Derives the display name for a directory being added to a workspace whose
/// existing display names are `existing`: the final path segment, with the
/// same two-level collision disambiguation the boot-time correction uses
/// (`name (parent)` then `name (full-prefix)`), so no two directories in one
/// workspace ever share a display name. Only the newcomer is renamed —
/// existing names stay stable. Returns `None` when the path has no usable
/// final segment.
pub(crate) fn derive_display_name(abs_path: &str, existing: &[String]) -> Option<String> {
    let (display, prefix) = split_display_name(abs_path)?;
    if !existing.iter().any(|n| n == &display) {
        return Some(display);
    }
    let level1 = match parent_segment(&prefix) {
        Some(parent) => format!("{} ({})", display, parent),
        None => display.clone(),
    };
    if !existing.iter().any(|n| n == &level1) {
        return Some(level1);
    }
    if !prefix.is_empty() {
        return Some(format!("{} ({})", display, prefix));
    }
    // Unreachable in practice: a colliding bare display with an empty prefix
    // implies an identical abs_path, which the UNIQUE constraint forbids.
    Some(level1)
}

/// Resolves same-display-name collisions within one workspace. Two levels:
/// first `name (parent-segment)`; if still ambiguous, `name (full-prefix)` —
/// the full prefix is guaranteed unique because `abs_path` is unique within
/// the workspace and the trailing segment is identical across the group.
fn disambiguate(rows: &[(DirectoryRow, String, String)]) -> Vec<String> {
    let mut names: Vec<String> = rows.iter().map(|(_, display, _)| display.clone()).collect();

    // Collect indices per display name to find collision groups.
    let mut groups: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (idx, (_, display, _)) in rows.iter().enumerate() {
        groups.entry(display.as_str()).or_default().push(idx);
    }
    for (_, indices) in groups {
        if indices.len() < 2 {
            continue;
        }
        // Level 1: append the parent segment, e.g. `foo (a)`. A row whose
        // prefix is empty keeps the bare name (cannot collide within the
        // group: identical bare display + empty prefix would mean identical
        // abs_path, which the UNIQUE constraint forbids).
        let level1: Vec<String> = indices
            .iter()
            .map(|&i| match parent_segment(&rows[i].2) {
                Some(parent) => format!("{} ({})", rows[i].1, parent),
                None => rows[i].1.clone(),
            })
            .collect();
        let unique = {
            let mut seen = std::collections::BTreeSet::new();
            level1.iter().all(|s| seen.insert(s.clone()))
        };
        if unique {
            for (&slot, name) in indices.iter().zip(level1) {
                names[slot] = name;
            }
        } else {
            // Level 2: append the full prefix — unique by construction.
            for &slot in &indices {
                let (_, display, prefix) = &rows[slot];
                if !prefix.is_empty() {
                    names[slot] = format!("{} ({})", display, prefix);
                }
            }
        }
    }
    names
}

/// Returns the final segment of a path prefix, e.g. `C:\proj\a` → `a`.
fn parent_segment(prefix: &str) -> Option<String> {
    let trimmed = prefix.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return None;
    }
    Some(match trimmed.rfind(['/', '\\']) {
        Some(pos) => trimmed[pos + 1..].to_string(),
        None => trimmed.to_string(),
    })
}

fn reparent_migrated_sessions(
    conn: &Connection,
    new_workspace_id: &str,
) -> Result<(), MigrationOrchestrationError> {
    conn.execute(
        "UPDATE sessions SET workspace_id = ?1 WHERE workspace_id = ?2",
        rusqlite::params![new_workspace_id, MIGRATED_PLACEHOLDER],
    )
    .map_err(StoreError::Sqlite)?;
    Ok(())
}

/// Reparents tasks and runs carrying the `__migrated__` placeholder workspace_id
/// to the real workspace id. Idempotent — a second run finds no rows to update.
fn reparent_migrated_tasks_runs(
    conn: &Connection,
    new_workspace_id: &str,
) -> Result<(), MigrationOrchestrationError> {
    conn.execute(
        "UPDATE tasks SET workspace_id = ?1 WHERE workspace_id = ?2",
        rusqlite::params![new_workspace_id, MIGRATED_PLACEHOLDER],
    )
    .map_err(StoreError::Sqlite)?;
    conn.execute(
        "UPDATE runs SET workspace_id = ?1 WHERE workspace_id = ?2",
        rusqlite::params![new_workspace_id, MIGRATED_PLACEHOLDER],
    )
    .map_err(StoreError::Sqlite)?;
    Ok(())
}

/// Ensures the legacy `is_active = 1` workspace is present in
/// `workspace_open_state` (migration 0024 backfill). Idempotent. Called on
/// every boot to cover databases created before 0024 was applied. Seeds only
/// when the open set is completely empty: under the v2 single-open model
/// (ADR-0021) this boot pass must never add a second row, so a stale
/// `is_active` flag next to an already-open workspace is ignored.
fn ensure_active_in_open_set(conn: &Connection) -> Result<(), MigrationOrchestrationError> {
    let now = crate::domain::now_ms();
    conn.execute(
        "INSERT INTO workspace_open_state (workspace_id, opened_at, last_focused_at, is_focused)
         SELECT id, ?1, ?1, 1
         FROM workspaces
         WHERE is_active = 1
           AND NOT EXISTS (SELECT 1 FROM workspace_open_state)",
        rusqlite::params![now],
    )
    .map_err(StoreError::Sqlite)?;
    Ok(())
}

/// Reclaims `__migrated__` tasks/runs by binding them to the currently focused
/// workspace (if any). Called on boot when no legacy root is present but the
/// registry is non-empty. Idempotent — a second run finds no rows to update.
fn reclaim_migrated_tasks_runs(conn: &Connection) -> Result<(), MigrationOrchestrationError> {
    conn.execute(
        "UPDATE tasks SET workspace_id = (
             SELECT workspace_id FROM workspace_open_state WHERE is_focused = 1
         ) WHERE workspace_id = ?1
           AND EXISTS (SELECT 1 FROM workspace_open_state WHERE is_focused = 1)",
        rusqlite::params![MIGRATED_PLACEHOLDER],
    )
    .map_err(StoreError::Sqlite)?;
    conn.execute(
        "UPDATE runs SET workspace_id = (
             SELECT workspace_id FROM workspace_open_state WHERE is_focused = 1
         ) WHERE workspace_id = ?1
           AND EXISTS (SELECT 1 FROM workspace_open_state WHERE is_focused = 1)",
        rusqlite::params![MIGRATED_PLACEHOLDER],
    )
    .map_err(StoreError::Sqlite)?;
    Ok(())
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

    #[test]
    fn needs_setup_when_no_legacy_and_empty_registry() {
        let conn = db();
        assert_eq!(run_if_needed(&conn).unwrap(), MigrationOutcome::NeedsSetup);
    }

    #[test]
    fn skipped_when_no_legacy_but_registry_nonempty() {
        let conn = db();
        // Manually insert a real workspace entry.
        let entry = WorkspaceEntry {
            id: Uuid::now_v7().to_string(),
            root_path: "C:\\ws".into(),
            color_tag: "paper-yellow".into(),
            created_at: 1,
            is_active: true,
            is_pinned: false,
        };
        repo::insert(&conn, &entry).unwrap();
        repo::set_active(&conn, &entry.id).unwrap();
        assert_eq!(run_if_needed(&conn).unwrap(), MigrationOutcome::Skipped);
    }

    #[test]
    fn migrates_placeholder_from_legacy_root() {
        let conn = db();
        // Simulate 0016: legacy root + placeholder row.
        settings::set(&conn, settings::WORKSPACE_ROOT, "C:\\projects\\demo").unwrap();
        let placeholder = WorkspaceEntry {
            id: "abcdef0123456789abcdef0123456789".into(), // non-uuid
            root_path: "C:\\projects\\demo".into(),
            color_tag: "paper-yellow".into(),
            created_at: 1,
            is_active: true,
            is_pinned: false,
        };
        repo::insert(&conn, &placeholder).unwrap();
        // Add a session with __migrated__ placeholder.
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES ('s1','','1','1')",
            [],
        )
        .unwrap();
        let outcome = run_if_needed(&conn).unwrap();
        assert!(matches!(outcome, MigrationOutcome::Migrated { .. }));
        // Placeholder id replaced with a real uuid.
        let entries = repo::list(&conn).unwrap();
        assert!(is_valid_uuid(&entries[0].id));
        // Session reparented.
        let wid: String = conn
            .query_row("SELECT workspace_id FROM sessions WHERE id='s1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_ne!(wid, MIGRATED_PLACEHOLDER);
        assert!(is_valid_uuid(&wid));
    }

    #[test]
    fn idempotent_on_second_run() {
        let conn = db();
        settings::set(&conn, settings::WORKSPACE_ROOT, "C:\\ws").unwrap();
        let placeholder = WorkspaceEntry {
            id: "nonuuidplaceholder1234567890ab".into(),
            root_path: "C:\\ws".into(),
            color_tag: "paper-yellow".into(),
            created_at: 1,
            is_active: true,
            is_pinned: false,
        };
        repo::insert(&conn, &placeholder).unwrap();
        let first = run_if_needed(&conn).unwrap();
        assert!(matches!(first, MigrationOutcome::Migrated { .. }));
        // Second run: no placeholder left, real uuid row present → still Migrated
        // (legacy root still set) but no double-reparenting side effects.
        let second = run_if_needed(&conn).unwrap();
        assert!(matches!(second, MigrationOutcome::Migrated { .. }));
        // Only one workspace entry.
        assert_eq!(repo::list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn inserts_fresh_entry_when_legacy_root_but_no_placeholder() {
        let conn = db();
        settings::set(&conn, settings::WORKSPACE_ROOT, "C:\\fresh").unwrap();
        // Registry empty, no placeholder — should insert a real entry.
        let outcome = run_if_needed(&conn).unwrap();
        assert!(matches!(outcome, MigrationOutcome::Migrated { .. }));
        let entries = repo::list(&conn).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(is_valid_uuid(&entries[0].id));
        assert_eq!(entries[0].root_path, "C:\\fresh");
        assert!(entries[0].is_active);
    }

    // ---- 0036 work-directory placeholder correction

    /// Seeds a workspace row so directory rows satisfy the foreign key.
    fn seed_workspace(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO workspaces (id, root_path, color_tag, created_at, is_active, is_pinned)
             VALUES (?1, ?2, 'paper-yellow', 1, 0, 0)",
            rusqlite::params![id, format!("C:\\seed\\{id}")],
        )
        .unwrap();
    }

    /// Inserts a 0036-style placeholder directory row (temporary hex id,
    /// display_name = abs_path).
    fn placeholder_dir(conn: &Connection, id: &str, ws: &str, abs_path: &str) {
        conn.execute(
            "INSERT INTO work_directories (id, workspace_id, abs_path, display_name, mounted, added_at)
             VALUES (?1, ?2, ?3, ?3, 1, 42)",
            rusqlite::params![id, ws, abs_path],
        )
        .unwrap();
    }

    /// Looks a directory row up by its stable key (workspace + path) and
    /// returns `(id, display_name)` — the placeholder id is rewritten by the
    /// correction, so it cannot be used to find the row afterwards.
    fn directory_display(conn: &Connection, ws: &str, abs_path: &str) -> (String, String) {
        conn.query_row(
            "SELECT id, display_name FROM work_directories WHERE workspace_id = ?1 AND abs_path = ?2",
            rusqlite::params![ws, abs_path],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .unwrap()
    }

    #[test]
    fn migrate_work_directories_corrects_placeholders() {
        let conn = db();
        seed_workspace(&conn, "w1");
        placeholder_dir(&conn, "hexplaceholder000000000001", "w1", "C:\\proj\\alpha");
        let report = migrate_work_directories(&conn).unwrap();
        assert_eq!(
            report,
            WorkDirectoryCorrection {
                corrected: 1,
                skipped: 0
            }
        );
        let (id, display) = directory_display(&conn, "w1", "C:\\proj\\alpha");
        assert_ne!(id, "hexplaceholder000000000001", "id replaced with uuid");
        assert!(is_valid_uuid(&id));
        assert_eq!(display, "alpha");
        // mounted / added_at untouched by the correction.
        let (mounted, added_at): (i64, i64) = conn
            .query_row(
                "SELECT mounted, added_at FROM work_directories WHERE id = ?1",
                [&id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((mounted, added_at), (1, 42));
    }

    #[test]
    fn migrate_work_directories_handles_trailing_slash_and_mixed_separators() {
        let conn = db();
        seed_workspace(&conn, "w1");
        placeholder_dir(
            &conn,
            "hexplaceholder000000000002",
            "w1",
            "/home/user/beta/",
        );
        migrate_work_directories(&conn).unwrap();
        let (_, display) = directory_display(&conn, "w1", "/home/user/beta/");
        assert_eq!(display, "beta");
    }

    #[test]
    fn migrate_work_directories_disambiguates_same_names_per_workspace() {
        let conn = db();
        seed_workspace(&conn, "w1");
        seed_workspace(&conn, "w2");
        // Same trailing segment within one workspace, different parents.
        placeholder_dir(&conn, "hexplaceholder000000000003", "w1", "C:\\a\\foo");
        placeholder_dir(&conn, "hexplaceholder000000000004", "w1", "C:\\b\\foo");
        // Same name in another workspace must NOT be disambiguated.
        placeholder_dir(&conn, "hexplaceholder000000000005", "w2", "C:\\c\\foo");
        let report = migrate_work_directories(&conn).unwrap();
        assert_eq!(report.corrected, 3);
        let d3 = directory_display(&conn, "w1", "C:\\a\\foo").1;
        let d4 = directory_display(&conn, "w1", "C:\\b\\foo").1;
        let d5 = directory_display(&conn, "w2", "C:\\c\\foo").1;
        assert_eq!(d3, "foo (a)");
        assert_eq!(d4, "foo (b)");
        assert_eq!(d5, "foo", "other workspace keeps the bare name");
    }

    #[test]
    fn migrate_work_directories_escalates_to_full_prefix_when_parent_collides() {
        let conn = db();
        seed_workspace(&conn, "w1");
        // Parent segments collide (`x`), full prefixes differ.
        placeholder_dir(&conn, "hexplaceholder000000000006", "w1", "C:\\a\\x\\foo");
        placeholder_dir(&conn, "hexplaceholder000000000007", "w1", "C:\\b\\x\\foo");
        migrate_work_directories(&conn).unwrap();
        let d6 = directory_display(&conn, "w1", "C:\\a\\x\\foo").1;
        let d7 = directory_display(&conn, "w1", "C:\\b\\x\\foo").1;
        assert_eq!(d6, "foo (C:\\a\\x)");
        assert_eq!(d7, "foo (C:\\b\\x)");
    }

    #[test]
    fn migrate_work_directories_skips_invalid_path_and_reports_event() {
        let conn = db();
        seed_workspace(&conn, "w1");
        placeholder_dir(&conn, "hexplaceholder000000000008", "w1", "");
        let report = migrate_work_directories(&conn).unwrap();
        assert_eq!(
            report,
            WorkDirectoryCorrection {
                corrected: 0,
                skipped: 1
            }
        );
        // Original row preserved untouched.
        let (id, display): (String, String) = conn
            .query_row(
                "SELECT id, display_name FROM work_directories WHERE workspace_id = 'w1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(id, "hexplaceholder000000000008");
        assert_eq!(display, "");
        // Audit event appended.
        let kinds: Vec<String> = conn
            .prepare("SELECT kind FROM events WHERE aggregate_type = 'workspace' AND aggregate_id = 'w1'")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(kinds.iter().any(|k| k == "migration_report"));
    }

    #[test]
    fn migrate_work_directories_is_idempotent() {
        let conn = db();
        seed_workspace(&conn, "w1");
        placeholder_dir(&conn, "hexplaceholder000000000009", "w1", "C:\\proj\\gamma");
        let first = migrate_work_directories(&conn).unwrap();
        assert_eq!(first.corrected, 1);
        // Second pass: no placeholder rows remain — no-op.
        let second = migrate_work_directories(&conn).unwrap();
        assert_eq!(second, WorkDirectoryCorrection::default());
        let count: i64 = conn
            .query_row("SELECT count(*) FROM work_directories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn run_if_needed_reparents_work_directories_on_placeholder_correction() {
        let conn = db();
        // Legacy scenario: placeholder workspace row + 0036 backfilled a
        // directory row under the placeholder workspace id.
        settings::set(&conn, settings::WORKSPACE_ROOT, "C:\\legacy").unwrap();
        let placeholder = WorkspaceEntry {
            id: "legacyplaceholder000000000001".into(),
            root_path: "C:\\legacy".into(),
            color_tag: "paper-yellow".into(),
            created_at: 1,
            is_active: true,
            is_pinned: false,
        };
        repo::insert(&conn, &placeholder).unwrap();
        placeholder_dir(
            &conn,
            "hexdir00000000000000000001",
            &placeholder.id,
            "C:\\legacy",
        );

        let outcome = run_if_needed(&conn).unwrap();
        assert!(matches!(outcome, MigrationOutcome::Migrated { .. }));

        // The directory row follows the rewritten workspace id and got its
        // uuid + display name corrected in the same boot pass.
        let (dir_ws, dir_id, display): (String, String, String) = conn
            .query_row(
                "SELECT workspace_id, id, display_name FROM work_directories",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        let ws = repo::list(&conn).unwrap();
        assert_eq!(ws.len(), 1);
        assert_eq!(
            dir_ws, ws[0].id,
            "directory follows the corrected workspace id"
        );
        assert!(is_valid_uuid(&dir_id));
        assert_eq!(display, "legacy");
    }

    // ---- derive_display_name (incremental add path, consumed by registry v2)

    #[test]
    fn derive_display_name_bare_when_no_collision() {
        assert_eq!(derive_display_name("C:\\a\\foo", &[]).unwrap(), "foo");
        assert_eq!(
            derive_display_name("C:\\a\\foo", &["bar".into(), "baz".into()]).unwrap(),
            "foo"
        );
        // Trailing slash and mixed separators.
        assert_eq!(
            derive_display_name("/home/user/beta/", &["foo".into()]).unwrap(),
            "beta"
        );
    }

    #[test]
    fn derive_display_name_escalates_through_two_levels() {
        // Level 1: parent segment suffix.
        assert_eq!(
            derive_display_name("C:\\b\\foo", &["foo".into()]).unwrap(),
            "foo (b)"
        );
        // Level 2: parent collides with an existing level-1 name.
        assert_eq!(
            derive_display_name("C:\\a\\foo", &["foo".into(), "foo (a)".into()]).unwrap(),
            "foo (C:\\a)"
        );
    }

    #[test]
    fn derive_display_name_unparseable_returns_none() {
        assert!(derive_display_name("", &[]).is_none());
        assert!(derive_display_name("\\\\\\", &[]).is_none());
    }

    // ---- boot-time open-set seeding respects single-open (ADR-0021)

    #[test]
    fn boot_seed_skips_when_open_set_nonempty() {
        let conn = db();
        // Stale legacy state: A still flagged active, B already open.
        seed_workspace(&conn, "wa");
        seed_workspace(&conn, "wb");
        conn.execute("UPDATE workspaces SET is_active = 1 WHERE id = 'wa'", [])
            .unwrap();
        conn.execute(
            "INSERT INTO workspace_open_state (workspace_id, opened_at, last_focused_at, is_focused)
             VALUES ('wb', 1, 1, 1)",
            [],
        )
        .unwrap();

        let outcome = run_if_needed(&conn).unwrap();
        assert!(matches!(outcome, MigrationOutcome::Skipped));
        let open_ids: Vec<String> = conn
            .prepare("SELECT workspace_id FROM workspace_open_state")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        // 单开：不再为 stale is_active 补行
        assert_eq!(open_ids, vec!["wb".to_string()]);
    }

    #[test]
    fn boot_seed_fills_empty_open_set_with_active_workspace() {
        let conn = db();
        seed_workspace(&conn, "wa");
        conn.execute("UPDATE workspaces SET is_active = 1 WHERE id = 'wa'", [])
            .unwrap();
        let outcome = run_if_needed(&conn).unwrap();
        assert!(matches!(outcome, MigrationOutcome::Skipped));
        let focused: String = conn
            .query_row(
                "SELECT workspace_id FROM workspace_open_state WHERE is_focused = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(focused, "wa");
    }
}
