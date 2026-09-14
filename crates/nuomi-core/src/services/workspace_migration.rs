//! Legacy single-workspace migration orchestration.
//!
//! On boot, after migrations 0016/0017 run, this reads the legacy
//! `app_settings.workspace_root` key and, if a placeholder row exists in the
//! `workspaces` table (inserted by 0016), corrects it with a real uuid-v7 id,
//! canonicalized path and hash-derived color. Sessions carrying the
//! `__migrated__` placeholder workspace_id are then reparented to the real id.
//! Finally the active workspace's codebase-memory products are migrated.

use std::path::PathBuf;

use rusqlite::Connection;
use thiserror::Error;
use uuid::Uuid;

use super::codebase_memory_migrator;
use super::workspace_palette;
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
            codebase_memory_migrator::MigrationError::Io(io) => {
                MigrationOrchestrationError::Io(io)
            }
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
    let legacy_root = settings::get(conn, settings::WORKSPACE_ROOT)?;
    let existing = repo::list(conn)?;

    match (legacy_root, existing.is_empty()) {
        (None, true) => return Ok(MigrationOutcome::NeedsSetup),
        (None, false) => return Ok(MigrationOutcome::Skipped),
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
                };
                repo::insert(conn, &entry)?;
                if entry.is_active {
                    repo::set_active(conn, &entry.id)?;
                }
                entry.id
            };

            // Reparent sessions carrying the placeholder workspace_id.
            reparent_migrated_sessions(conn, &migrated_id)?;

            // Migrate codebase-memory products for the active workspace.
            if let Some(active) = repo::find_active(conn)? {
                let root = PathBuf::from(&active.root_path);
                if root.is_dir() {
                    let _ = codebase_memory_migrator::migrate(&root)?;
                }
            }

            Ok(MigrationOutcome::Migrated { workspace_id: migrated_id })
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
    conn.execute(
        "UPDATE workspaces SET id = ?1, root_path = ?2, color_tag = ?3 WHERE id = ?4",
        rusqlite::params![new_id, root_path, color, old_id],
    )
    .map_err(StoreError::Sqlite)?;
    Ok(())
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
            .query_row("SELECT workspace_id FROM sessions WHERE id='s1'", [], |r| r.get(0))
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
}
