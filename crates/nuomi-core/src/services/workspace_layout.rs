//! Workspace layout management service.
//!
//! Captures and restores the workspace layout (open set, focused workspace,
//! split-screen configuration) across restarts.

use std::path::PathBuf;

use rusqlite::Connection;
use thiserror::Error;

use super::workspace_open_set::{OpenSetError, WorkspaceOpenSetService};
use crate::store::repos::workspaces;
use crate::store::repos::workspace_layout_snapshot;
use crate::store::repos::workspace_layout_snapshot::{LayoutMode, LayoutSnapshotRow};
use crate::store::repos::workspace_open_state;
use crate::store::repos::events;
use crate::store::{migrations, Db, StoreError};

#[derive(Debug, Error)]
pub enum LayoutError {
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("open-set error: {0}")]
    OpenSet(#[from] super::workspace_open_set::OpenSetError),
}

/// The layout service. Holds the db path; each call opens a fresh connection.
#[derive(Clone)]
pub struct WorkspaceLayoutService {
    db_path: PathBuf,
}

impl WorkspaceLayoutService {
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    fn conn(&self) -> Result<Connection, LayoutError> {
        let db = Db::open(&self.db_path.to_string_lossy())?;
        migrations::run(&db.0)?;
        Ok(db.0)
    }

    /// Captures the current layout: open set + focused workspace + split config.
    /// Writes a single-row snapshot (overwriting any previous snapshot).
    pub fn capture_snapshot(
        &self,
        mode: LayoutMode,
        split_workspace_ids: Option<[String; 2]>,
    ) -> Result<(), LayoutError> {
        let conn = self.conn()?;
        let focused = workspace_open_state::find_focused(&conn)?;
        let now = crate::domain::now_ms();
        workspace_layout_snapshot::upsert(
            &conn,
            &LayoutSnapshotRow {
                mode,
                split_workspace_ids,
                focused_workspace_id: focused.map(|r| r.workspace_id),
                captured_at: now,
            },
        )?;
        Ok(())
    }

    /// Restores the layout from the snapshot. Pinned workspaces are always
    /// restored; non-pinned workspaces are restored when `restore_non_pinned`
    /// is true. Directory-missing workspaces are skipped. Focus and split
    /// config are restored if the referenced workspaces are still open.
    pub fn restore_snapshot(&self, restore_non_pinned: bool) -> Result<RestoreOutcome, LayoutError> {
        let conn = self.conn()?;
        let snapshot = workspace_layout_snapshot::get(&conn)?;

        match snapshot {
            Some(snap) => self.restore_from_snapshot(&conn, &snap, restore_non_pinned),
            None => self.restore_default(&conn),
        }
    }

    fn restore_from_snapshot(
        &self,
        conn: &Connection,
        snap: &LayoutSnapshotRow,
        restore_non_pinned: bool,
    ) -> Result<RestoreOutcome, LayoutError> {
        let open_svc = WorkspaceOpenSetService::new(self.db_path.clone());
        let all_workspaces = workspaces::list(conn)?;

        // Restore pinned workspaces (always) + non-pinned (if configured).
        let mut restored_ids = Vec::new();
        for ws in &all_workspaces {
            if ws.is_pinned || restore_non_pinned {
                match open_svc.open(&ws.id) {
                    Ok(()) => restored_ids.push(ws.id.clone()),
                    Err(OpenSetError::DirectoryMissing(_)) => {
                        let _ = events::append(
                            conn,
                            "workspace",
                            &ws.id,
                            "workspace.directory_missing",
                            &serde_json::json!({}),
                            crate::domain::now_ms(),
                        );
                    }
                    Err(_) => {}
                }
            }
        }

        // Restore focus (snapshot's focused_workspace_id if still open, else first open).
        let open_state = workspace_open_state::list(conn)?;
        let focused_id = if let Some(ref snap_focused) = snap.focused_workspace_id {
            if open_state.iter().any(|r| &r.workspace_id == snap_focused) {
                Some(snap_focused.clone())
            } else {
                open_state.first().map(|r| r.workspace_id.clone())
            }
        } else {
            open_state.first().map(|r| r.workspace_id.clone())
        };

        if let Some(ref fid) = focused_id {
            let _ = workspace_open_state::set_focused(conn, fid);
        }

        // Restore split if both workspace ids are still open.
        let split = snap.split_workspace_ids.as_ref().and_then(|[a, b]| {
            let open_ids: Vec<&String> = open_state.iter().map(|r| &r.workspace_id).collect();
            if open_ids.contains(&a) && open_ids.contains(&b) {
                Some([a.clone(), b.clone()])
            } else {
                None
            }
        });

        let mode = if split.is_some() {
            snap.mode
        } else {
            LayoutMode::Single
        };

        Ok(RestoreOutcome {
            restored_workspace_ids: restored_ids,
            focused_workspace_id: focused_id,
            mode,
            split_workspace_ids: split,
        })
    }

    fn restore_default(&self, conn: &Connection) -> Result<RestoreOutcome, LayoutError> {
        // No snapshot: restore pinned workspaces as the open set, focus first pinned.
        let open_svc = WorkspaceOpenSetService::new(self.db_path.clone());
        let all_workspaces = workspaces::list(conn)?;

        let mut restored_ids = Vec::new();
        for ws in &all_workspaces {
            if ws.is_pinned {
                if let Ok(()) = open_svc.open(&ws.id) {
                    restored_ids.push(ws.id.clone());
                }
            }
        }

        let open_state = workspace_open_state::list(conn)?;
        let focused_id = open_state.first().map(|r| r.workspace_id.clone());
        if let Some(ref fid) = focused_id {
            let _ = workspace_open_state::set_focused(conn, fid);
        }

        Ok(RestoreOutcome {
            restored_workspace_ids: restored_ids,
            focused_workspace_id: focused_id,
            mode: LayoutMode::Single,
            split_workspace_ids: None,
        })
    }
}

/// Outcome of a layout restore operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreOutcome {
    pub restored_workspace_ids: Vec<String>,
    pub focused_workspace_id: Option<String>,
    pub mode: LayoutMode,
    pub split_workspace_ids: Option<[String; 2]>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::workspace_registry::WorkspaceRegistry;
    use std::fs;
    use std::path::Path;

    fn setup() -> (WorkspaceLayoutService, WorkspaceRegistry, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let svc = WorkspaceLayoutService::new(db_path.clone());
        let reg = WorkspaceRegistry::new(db_path);
        (svc, reg, dir)
    }

    fn make_ws_dir(parent: &Path, name: &str) -> PathBuf {
        let p = parent.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn capture_and_restore_roundtrip() {
        let (svc, reg, dir) = setup();
        let a = make_ws_dir(dir.path(), "a");
        let b = make_ws_dir(dir.path(), "b");
        let ea = reg.register(&a).unwrap();
        let eb = reg.register(&b).unwrap();

        let open_svc = WorkspaceOpenSetService::new(dir.path().join("test.db"));
        open_svc.open(&ea.id).unwrap();
        open_svc.open(&eb.id).unwrap();

        svc.capture_snapshot(LayoutMode::Split, Some([ea.id.clone(), eb.id.clone()]))
            .unwrap();

        // Close all, then restore.
        open_svc.close_all(false).unwrap();
        let outcome = svc.restore_snapshot(true).unwrap();
        assert!(!outcome.restored_workspace_ids.is_empty());
    }

    #[test]
    fn restore_with_no_snapshot_restores_pinned_only() {
        let (svc, reg, dir) = setup();
        let a = make_ws_dir(dir.path(), "a");
        let ea = reg.register(&a).unwrap();
        // Pin workspace a.
        {
            let db = Db::open(&dir.path().join("test.db").to_string_lossy()).unwrap();
            migrations::run(&db.0).unwrap();
            workspaces::pin(&db.0, &ea.id).unwrap();
        }
        let outcome = svc.restore_snapshot(false).unwrap();
        assert!(outcome.restored_workspace_ids.contains(&ea.id));
    }

    #[test]
    fn restore_skips_directory_missing() {
        let (svc, reg, dir) = setup();
        let a = make_ws_dir(dir.path(), "a");
        let ea = reg.register(&a).unwrap();
        // Remove directory.
        fs::remove_dir_all(&a).unwrap();
        let outcome = svc.restore_snapshot(true).unwrap();
        assert!(!outcome.restored_workspace_ids.contains(&ea.id));
    }
}
