//! Workspace open-set management service.
//!
//! Manages which workspaces are currently open (the "open set") and which one
//! is focused. This replaces the legacy single-active-workspace model with a
//! multi-open + single-focused model, enabling parallel workspace execution.

use std::path::PathBuf;

use rusqlite::Connection;
use thiserror::Error;

use crate::store::repos::events;
use crate::store::repos::settings;
use crate::store::repos::workspaces;
use crate::store::repos::workspace_open_state;
use crate::store::repos::workspace_open_state::WorkspaceOpenStateRow;
use crate::store::repos::workspace_recent;
use crate::store::{migrations, Db, StoreError};

/// Maximum number of workspaces that can be open simultaneously.
pub const MAX_OPEN_WORKSPACES: usize = 8;

/// Timeout for probing directory reachability (milliseconds).
pub const DIRECTORY_PROBE_TIMEOUT_MS: u64 = 2000;

/// Errors emitted by the open-set service.
#[derive(Debug, Error)]
pub enum OpenSetError {
    #[error("workspace not found: {0}")]
    NotFound(String),
    #[error("workspace already open: {0}")]
    AlreadyOpen(String),
    #[error("open set is full (max {0})")]
    OpenSetFull(usize),
    #[error("workspace directory missing or unreadable: {0}")]
    DirectoryMissing(String),
    #[error("directory probe timed out after {0}ms")]
    ProbeTimeout(u64),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("close requires confirmation: {reason}")]
    NeedConfirm {
        workspace_id: String,
        reason: String,
        dirty_files: Vec<String>,
        running_tasks: Vec<String>,
    },
}

/// Provider trait for the open set — consumed by the orchestrator to query
/// which workspaces are open and which is focused, without depending on the
/// concrete service (testable with stubs).
pub trait OpenSetProvider {
    /// Returns the ids of all open workspaces.
    fn open_ids(&self) -> Result<Vec<String>, OpenSetError>;

    /// Returns the focused workspace id, if any.
    fn focused_id(&self) -> Result<Option<String>, OpenSetError>;
}

/// The open-set service. Holds the db path; each call opens a fresh connection.
#[derive(Clone)]
pub struct WorkspaceOpenSetService {
    db_path: PathBuf,
}

impl WorkspaceOpenSetService {
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    fn conn(&self) -> Result<Connection, OpenSetError> {
        let db = Db::open(&self.db_path.to_string_lossy())?;
        migrations::run(&db.0)?;
        Ok(db.0)
    }

    /// Opens a workspace: adds it to the open set and focuses it.
    /// Idempotent — opening an already-open workspace just refocuses it.
    pub fn open(&self, workspace_id: &str) -> Result<(), OpenSetError> {
        let conn = self.conn()?;
        let entry = workspaces::find_by_id(&conn, workspace_id)?
            .ok_or_else(|| OpenSetError::NotFound(workspace_id.to_string()))?;

        // Idempotent: if already open, just refocus.
        let existing = workspace_open_state::list(&conn)?;
        if existing.iter().any(|r| r.workspace_id == workspace_id) {
            workspace_open_state::set_focused(&conn, workspace_id)?;
            emit_event(&conn, workspace_id, "workspace.focused");
            return Ok(());
        }

        // Check capacity (configurable via app_settings, default MAX_OPEN_WORKSPACES).
        let max_open = settings::get(&conn, settings::MAX_OPEN_WORKSPACES_KEY)
            .ok()
            .and_then(|v| v)
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(MAX_OPEN_WORKSPACES);
        if existing.len() >= max_open {
            return Err(OpenSetError::OpenSetFull(max_open));
        }

        // Probe directory reachability.
        if !std::path::Path::new(&entry.root_path).is_dir() {
            return Err(OpenSetError::DirectoryMissing(entry.root_path.clone()));
        }

        let now = crate::domain::now_ms();
        let is_first = existing.is_empty();
        workspace_open_state::insert(
            &conn,
            &WorkspaceOpenStateRow {
                workspace_id: workspace_id.to_string(),
                opened_at: now,
                last_focused_at: now,
                is_focused: is_first,
            },
        )?;

        if !is_first {
            workspace_open_state::set_focused(&conn, workspace_id)?;
        }

        workspace_recent::touch(&conn, workspace_id, now, entry.is_pinned)?;
        emit_event(&conn, workspace_id, "workspace.opened");
        emit_event(&conn, workspace_id, "workspace.focused");
        Ok(())
    }

    /// Closes a workspace: removes it from the open set. If `force` is false
    /// and there are unsaved edits or running tasks, returns `NeedConfirm`.
    /// Focus transfers to the most-recently-focused remaining workspace.
    pub fn close(&self, workspace_id: &str, force: bool) -> Result<CloseOutcome, OpenSetError> {
        let conn = self.conn()?;
        let state = workspace_open_state::list(&conn)?;
        let row = state
            .iter()
            .find(|r| r.workspace_id == workspace_id)
            .ok_or_else(|| OpenSetError::NotFound(workspace_id.to_string()))?;

        if !force {
            let dirty_files = find_dirty_files(&conn, workspace_id)?;
            let running_tasks = find_running_tasks(&conn, workspace_id)?;
            if !dirty_files.is_empty() || !running_tasks.is_empty() {
                return Err(OpenSetError::NeedConfirm {
                    workspace_id: workspace_id.to_string(),
                    reason: "unsaved edits or running tasks".into(),
                    dirty_files,
                    running_tasks,
                });
            }
        }

        let was_focused = row.is_focused;
        workspace_open_state::remove(&conn, workspace_id)?;
        emit_event(&conn, workspace_id, "workspace.closed");

        let new_focused = if was_focused {
            let remaining = workspace_open_state::list(&conn)?;
            if let Some(first) = remaining.first() {
                workspace_open_state::set_focused(&conn, &first.workspace_id)?;
                emit_event(&conn, &first.workspace_id, "workspace.focused");
                Some(first.workspace_id.clone())
            } else {
                None
            }
        } else {
            None
        };

        Ok(CloseOutcome {
            closed_id: workspace_id.to_string(),
            new_focused_id: new_focused,
        })
    }

    /// Focuses a workspace (must already be open).
    pub fn focus(&self, workspace_id: &str) -> Result<(), OpenSetError> {
        let conn = self.conn()?;
        let state = workspace_open_state::list(&conn)?;
        if !state.iter().any(|r| r.workspace_id == workspace_id) {
            return Err(OpenSetError::NotFound(workspace_id.to_string()));
        }
        let now = crate::domain::now_ms();
        workspace_open_state::set_focused(&conn, workspace_id)?;
        workspace_open_state::touch_last_focused(&conn, workspace_id, now)?;
        workspace_recent::touch(&conn, workspace_id, now, false)?;
        emit_event(&conn, workspace_id, "workspace.focused");
        Ok(())
    }

    /// Closes all open workspaces. If `exclude_pinned` is true, pinned
    /// workspaces remain open. Focus transfers to the most-recently-focused
    /// remaining workspace (if any).
    pub fn close_all(&self, exclude_pinned: bool) -> Result<Vec<CloseOutcome>, OpenSetError> {
        let conn = self.conn()?;
        let state = workspace_open_state::list(&conn)?;
        let mut outcomes = Vec::new();

        let to_close: Vec<String> = if exclude_pinned {
            state
                .iter()
                .filter_map(|r| {
                    let entry = workspaces::find_by_id(&conn, &r.workspace_id).ok().flatten()?;
                    if entry.is_pinned {
                        None
                    } else {
                        Some(r.workspace_id.clone())
                    }
                })
                .collect()
        } else {
            state.iter().map(|r| r.workspace_id.clone()).collect()
        };

        for id in to_close {
            let outcome = self.close(&id, true)?;
            outcomes.push(outcome);
        }
        Ok(outcomes)
    }

    /// Returns the current open set as DTO rows.
    pub fn list_open(&self) -> Result<Vec<WorkspaceOpenStateRow>, OpenSetError> {
        let conn = self.conn()?;
        Ok(workspace_open_state::list(&conn)?)
    }

    /// Returns the focused workspace id, if any.
    pub fn focused_id(&self) -> Result<Option<String>, OpenSetError> {
        let conn = self.conn()?;
        Ok(workspace_open_state::find_focused(&conn)?.map(|r| r.workspace_id))
    }
}

impl OpenSetProvider for WorkspaceOpenSetService {
    fn open_ids(&self) -> Result<Vec<String>, OpenSetError> {
        Ok(self.list_open()?.into_iter().map(|r| r.workspace_id).collect())
    }

    fn focused_id(&self) -> Result<Option<String>, OpenSetError> {
        self.focused_id()
    }
}

/// Outcome of a close operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseOutcome {
    pub closed_id: String,
    pub new_focused_id: Option<String>,
}

fn emit_event(conn: &Connection, workspace_id: &str, kind: &str) {
    let now = crate::domain::now_ms();
    let _ = events::append(
        conn,
        "workspace",
        workspace_id,
        kind,
        &serde_json::json!({}),
        now,
    );
}

fn find_dirty_files(
    _conn: &Connection,
    _workspace_id: &str,
) -> Result<Vec<String>, OpenSetError> {
    // TODO: integrate with editor dirty-state tracking (ephemeral, not persisted).
    Ok(Vec::new())
}

fn find_running_tasks(
    conn: &Connection,
    workspace_id: &str,
) -> Result<Vec<String>, OpenSetError> {
    use crate::store::repos::tasks_runs;
    let runs = tasks_runs::list_active_runs_by_workspace(conn, workspace_id)?;
    Ok(runs.into_iter().map(|r| r.id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::workspace_registry::WorkspaceRegistry;
    use std::fs;
    use std::path::Path;

    fn setup() -> (WorkspaceOpenSetService, WorkspaceRegistry, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let svc = WorkspaceOpenSetService::new(db_path.clone());
        let reg = WorkspaceRegistry::new(db_path);
        (svc, reg, dir)
    }

    fn make_ws_dir(parent: &Path, name: &str) -> PathBuf {
        let p = parent.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn open_first_workspace_auto_focuses() {
        let (svc, reg, dir) = setup();
        let ws = make_ws_dir(dir.path(), "a");
        let entry = reg.register(&ws).unwrap();
        svc.open(&entry.id).unwrap();
        let focused = svc.focused_id().unwrap().unwrap();
        assert_eq!(focused, entry.id);
        assert_eq!(svc.list_open().unwrap().len(), 1);
    }

    #[test]
    fn open_second_workspace_steals_focus() {
        let (svc, reg, dir) = setup();
        let a = make_ws_dir(dir.path(), "a");
        let b = make_ws_dir(dir.path(), "b");
        let ea = reg.register(&a).unwrap();
        let eb = reg.register(&b).unwrap();
        svc.open(&ea.id).unwrap();
        svc.open(&eb.id).unwrap();
        assert_eq!(svc.focused_id().unwrap().unwrap(), eb.id);
        assert_eq!(svc.list_open().unwrap().len(), 2);
    }

    #[test]
    fn open_is_idempotent() {
        let (svc, reg, dir) = setup();
        let ws = make_ws_dir(dir.path(), "a");
        let entry = reg.register(&ws).unwrap();
        svc.open(&entry.id).unwrap();
        svc.open(&entry.id).unwrap();
        assert_eq!(svc.list_open().unwrap().len(), 1);
    }

    #[test]
    fn open_missing_workspace_returns_not_found() {
        let (svc, _reg, _dir) = setup();
        assert!(matches!(svc.open("nope"), Err(OpenSetError::NotFound(_))));
    }

    #[test]
    fn open_exceeding_capacity_returns_full() {
        let (svc, reg, dir) = setup();
        let mut ids = Vec::new();
        for i in 0..(MAX_OPEN_WORKSPACES + 1) {
            let ws = make_ws_dir(dir.path(), &format!("ws{i}"));
            let entry = reg.register(&ws).unwrap();
            ids.push(entry.id);
        }
        for id in &ids[..MAX_OPEN_WORKSPACES] {
            svc.open(id).unwrap();
        }
        assert!(matches!(
            svc.open(&ids[MAX_OPEN_WORKSPACES]),
            Err(OpenSetError::OpenSetFull(_))
        ));
    }

    #[test]
    fn close_transfers_focus_to_most_recent() {
        let (svc, reg, dir) = setup();
        let a = make_ws_dir(dir.path(), "a");
        let b = make_ws_dir(dir.path(), "b");
        let ea = reg.register(&a).unwrap();
        let eb = reg.register(&b).unwrap();
        svc.open(&ea.id).unwrap();
        svc.open(&eb.id).unwrap();
        // eb is focused; close it → focus transfers to ea.
        let outcome = svc.close(&eb.id, true).unwrap();
        assert_eq!(outcome.new_focused_id, Some(ea.id.clone()));
        assert_eq!(svc.focused_id().unwrap().unwrap(), ea.id);
    }

    #[test]
    fn close_last_workspace_enters_empty() {
        let (svc, reg, dir) = setup();
        let ws = make_ws_dir(dir.path(), "a");
        let entry = reg.register(&ws).unwrap();
        svc.open(&entry.id).unwrap();
        let outcome = svc.close(&entry.id, true).unwrap();
        assert!(outcome.new_focused_id.is_none());
        assert!(svc.list_open().unwrap().is_empty());
    }

    #[test]
    fn focus_requires_open() {
        let (svc, reg, dir) = setup();
        let a = make_ws_dir(dir.path(), "a");
        let b = make_ws_dir(dir.path(), "b");
        let _ea = reg.register(&a).unwrap();
        let eb = reg.register(&b).unwrap();
        // b is registered but not open (only first workspace auto-opens).
        assert!(matches!(svc.focus(&eb.id), Err(OpenSetError::NotFound(_))));
    }

    #[test]
    fn close_all_closes_everything() {
        let (svc, reg, dir) = setup();
        let a = make_ws_dir(dir.path(), "a");
        let b = make_ws_dir(dir.path(), "b");
        let ea = reg.register(&a).unwrap();
        let eb = reg.register(&b).unwrap();
        svc.open(&ea.id).unwrap();
        svc.open(&eb.id).unwrap();
        let outcomes = svc.close_all(false).unwrap();
        assert_eq!(outcomes.len(), 2);
        assert!(svc.list_open().unwrap().is_empty());
    }
}
