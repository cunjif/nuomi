//! Workspace registry v2: two-level model service (ADR-0021).
//!
//! Single-open runtime over the workspaces → work_directories two-level
//! model. `workspace_open_state` is narrowed at the service layer to at most
//! one row — the 0024 table itself stays untouched (append-only); the legacy
//! `is_active` column is kept in sync with the open workspace so the boot
//! pass seeds the open set consistently.
//!
//! "Opening a directory" gains new semantics: no open workspace → the
//! directory's owning workspace is opened (a fresh one is created when the
//! path is unowned); open workspace → the directory joins it, moving in from
//! any other workspace (globally-unique ownership). Removing a workspace's
//! last directory auto-closes it (empty state) — the registry row, sessions
//! and on-disk files stay untouched.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use thiserror::Error;
use uuid::Uuid;

use super::codebase_memory_migrator;
use super::nuomi_dir;
use super::workspace_guard;
use super::workspace_label;
use super::workspace_palette;
use crate::store::repos::events;
use crate::store::repos::work_directories::{self, WorkDirectoryStatus};
use crate::store::repos::workspace_layout_snapshot::{self, LayoutMode, LayoutSnapshotRow};
use crate::store::repos::workspace_open_state::{self, WorkspaceOpenStateRow};
use crate::store::repos::workspace_recent;
use crate::store::repos::workspaces as repo;
use crate::store::repos::workspaces::WorkspaceEntry;
use crate::store::{migrations, Db, StoreError};

/// One active run of a workspace, as shown by the close-confirm dialog
/// (`close_preview`). `task_title` falls back to the task id when the task
/// row is gone (deleted task with an orphaned run).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RunningTaskSummary {
    pub run_id: String,
    pub task_title: String,
    pub status: crate::domain::run_state::RunState,
}

#[derive(Debug, Error)]
pub enum RegistryV2Error {
    #[error("path does not exist or is not a directory: {0}")]
    InvalidPath(String),
    #[error("path is blacklisted: {0}")]
    Blacklisted(String),
    #[error("directory missing on disk: {0}")]
    DirectoryMissing(String),
    #[error("workspace not found: {0}")]
    NotFound(String),
    #[error("directory not found for workspace: {0}")]
    DirectoryNotFound(String),
    #[error("another workspace is already open: {0}")]
    AlreadyOpen(String),
    #[error("workspace has no directories to open: {0}")]
    NoDirectories(String),
    /// Close refused without `force`: the workspace has active runs. The
    /// payload carries the run ids so callers (and users) can see what is
    /// still running before confirming. Unsaved editor edits are NOT part of
    /// this check — they are a frontend ephemeral fact guarded client-side.
    #[error("workspace '{workspace_id}' has running tasks: {running_tasks:?}")]
    NeedConfirm {
        workspace_id: String,
        running_tasks: Vec<String>,
    },
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<nuomi_dir::NuomiDirError> for RegistryV2Error {
    fn from(e: nuomi_dir::NuomiDirError) -> Self {
        match e {
            nuomi_dir::NuomiDirError::Conflict(s) => RegistryV2Error::Io(std::io::Error::other(s)),
            nuomi_dir::NuomiDirError::Io(io) => RegistryV2Error::Io(io),
        }
    }
}

impl From<codebase_memory_migrator::MigrationError> for RegistryV2Error {
    fn from(e: codebase_memory_migrator::MigrationError) -> Self {
        match e {
            codebase_memory_migrator::MigrationError::Io(io) => RegistryV2Error::Io(io),
        }
    }
}

/// A workspace with its directories and derived availability — the read model
/// returned by v2 operations. Workspace labels are derived separately via
/// [`workspace_label::derive_label`] (task 6.4) and will extend this view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceView {
    pub entry: WorkspaceEntry,
    pub directories: Vec<WorkDirectoryStatus>,
}

/// Result of removing a work directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoveDirectoryResult {
    /// True when the workspace's last directory was removed: the workspace
    /// auto-closes (open-state row cleared, registry row kept).
    pub workspace_closed: bool,
}

/// The v2 registry service. Holds the db path; each call opens a fresh
/// connection (matching the `commands.rs` pattern for blocking store access).
#[derive(Clone)]
pub struct WorkspaceRegistryV2 {
    db_path: PathBuf,
}

impl WorkspaceRegistryV2 {
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    fn conn(&self) -> Result<Connection, RegistryV2Error> {
        let db = Db::open(&self.db_path.to_string_lossy())?;
        migrations::run(&db.0)?;
        Ok(db.0)
    }

    /// "打开目录" new semantics (design B1): no open workspace → open the
    /// path's owning workspace, creating a fresh one when the path is unowned;
    /// open workspace → the directory joins the current workspace (mounted,
    /// globally-unique ownership moves it in). Returns the resulting view.
    pub fn add_directory(&self, path: &Path) -> Result<WorkspaceView, RegistryV2Error> {
        if workspace_guard::is_blacklisted(path) {
            return Err(RegistryV2Error::Blacklisted(
                path.to_string_lossy().to_string(),
            ));
        }
        if !path.is_dir() {
            return Err(RegistryV2Error::DirectoryMissing(
                path.to_string_lossy().to_string(),
            ));
        }
        let canonical = workspace_guard::canonicalize(path).map_err(|e| match e {
            workspace_guard::PathGuardError::InvalidPath(p) => RegistryV2Error::InvalidPath(p),
            workspace_guard::PathGuardError::Blacklisted(p) => RegistryV2Error::Blacklisted(p),
            workspace_guard::PathGuardError::Io(io) => RegistryV2Error::Io(io),
        })?;
        if workspace_guard::is_blacklisted(&canonical) {
            return Err(RegistryV2Error::Blacklisted(
                canonical.to_string_lossy().to_string(),
            ));
        }
        let abs_path = canonical.to_string_lossy().to_string();
        let conn = self.conn()?;

        if let Some(current) = current_open_id(&conn)? {
            // Open workspace: the directory joins it. A path already owned by
            // the current workspace is simply (re)mounted; any other path
            // moves in (repo-layer ownership transaction).
            if let Some(owned) = work_directories::find_by_path(&conn, &abs_path)?
                .filter(|d| d.workspace_id == current)
            {
                work_directories::set_mounted(&conn, &owned.id, true)?;
                emit_json(
                    &conn,
                    &current,
                    "workspace.directory_mounted",
                    serde_json::json!({ "directory_id": owned.id }),
                );
            } else {
                let display = derive_display(&conn, &current, &abs_path)?;
                work_directories::add(&conn, &current, &abs_path, &display)?;
            }
            return self.view_of(&conn, &current);
        }

        // Empty state: an already-owned path opens its owning workspace (no
        // silent theft from a closed workspace); an unowned path creates a
        // fresh workspace containing only this directory.
        if let Some(owned) = work_directories::find_by_path(&conn, &abs_path)? {
            work_directories::set_mounted(&conn, &owned.id, true)?;
            open_single(&conn, &owned.workspace_id)?;
            return self.view_of(&conn, &owned.workspace_id);
        }
        let display = workspace_label::derive_display_name(&abs_path, &[])
            .ok_or_else(|| RegistryV2Error::InvalidPath(abs_path.clone()))?;
        let id = Uuid::now_v7().to_string();
        let entry = WorkspaceEntry {
            // root_path is a legacy compatibility column (ADR-0021: new
            // workspaces stop writing meaningful values) — the workspace id
            // satisfies NOT NULL UNIQUE without colliding with real paths.
            root_path: id.clone(),
            id,
            color_tag: workspace_palette::color_for(&abs_path).0.to_string(),
            created_at: crate::domain::now_ms(),
            is_active: false,
            is_pinned: false,
        };
        nuomi_dir::ensure_nuomi_dir(&canonical)?;
        let _ = codebase_memory_migrator::migrate(&canonical)?;
        repo::insert(&conn, &entry)?;
        work_directories::add(&conn, &entry.id, &abs_path, &display)?;
        open_single(&conn, &entry.id)?;
        self.view_of(&conn, &entry.id)
    }

    /// Removes a work directory (design B2). Removing the workspace's last
    /// directory auto-closes it: the open-state row is cleared and the
    /// registry row, sessions and on-disk files stay untouched.
    pub fn remove_directory(
        &self,
        workspace_id: &str,
        directory_id: &str,
    ) -> Result<RemoveDirectoryResult, RegistryV2Error> {
        let conn = self.conn()?;
        let dir = work_directories::find_by_id(&conn, directory_id)?
            .filter(|d| d.workspace_id == workspace_id)
            .ok_or_else(|| RegistryV2Error::DirectoryNotFound(directory_id.to_string()))?;
        work_directories::remove(&conn, &dir.id)?;
        let remaining = work_directories::list_by_workspace(&conn, workspace_id)?;
        if !remaining.is_empty() {
            return Ok(RemoveDirectoryResult {
                workspace_closed: false,
            });
        }
        // Last directory removed → auto-close (state machine: Open → Empty).
        if current_open_id(&conn)?.as_deref() == Some(workspace_id) {
            workspace_open_state::remove(&conn, workspace_id)?;
            clear_active(&conn)?;
            emit(&conn, workspace_id, "workspace.closed");
        }
        Ok(RemoveDirectoryResult {
            workspace_closed: true,
        })
    }

    /// Mounts a work directory (back into the visible set).
    pub fn mount_directory(
        &self,
        workspace_id: &str,
        directory_id: &str,
    ) -> Result<(), RegistryV2Error> {
        self.set_mounted(workspace_id, directory_id, true)
    }

    /// Unmounts a work directory (out of the visible set; row kept).
    pub fn unmount_directory(
        &self,
        workspace_id: &str,
        directory_id: &str,
    ) -> Result<(), RegistryV2Error> {
        self.set_mounted(workspace_id, directory_id, false)
    }

    fn set_mounted(
        &self,
        workspace_id: &str,
        directory_id: &str,
        mounted: bool,
    ) -> Result<(), RegistryV2Error> {
        let conn = self.conn()?;
        let dir = work_directories::find_by_id(&conn, directory_id)?
            .filter(|d| d.workspace_id == workspace_id)
            .ok_or_else(|| RegistryV2Error::DirectoryNotFound(directory_id.to_string()))?;
        work_directories::set_mounted(&conn, &dir.id, mounted)?;
        let kind = if mounted {
            "workspace.directory_mounted"
        } else {
            "workspace.directory_unmounted"
        };
        emit_json(
            &conn,
            workspace_id,
            kind,
            serde_json::json!({ "directory_id": dir.id }),
        );
        Ok(())
    }

    /// Switches to another workspace (design B4): saves the outgoing
    /// workspace's layout snapshot (failure → warning event, switch
    /// continues), closes every open row (single-open), opens the target and
    /// focuses it.
    pub fn switch(&self, workspace_id: &str) -> Result<WorkspaceView, RegistryV2Error> {
        let conn = self.conn()?;
        let target = repo::find_by_id(&conn, workspace_id)?
            .ok_or_else(|| RegistryV2Error::NotFound(workspace_id.to_string()))?;
        if work_directories::list_by_workspace(&conn, workspace_id)?.is_empty() {
            return Err(RegistryV2Error::NoDirectories(workspace_id.to_string()));
        }
        if let Some(current) = current_open_id(&conn)? {
            if current != target.id {
                // Save the outgoing workspace's layout snapshot first. A
                // failure must not block the switch: record a warning event
                // and continue (spec 5.5.1-8a).
                let save = workspace_layout_snapshot::upsert(
                    &conn,
                    &LayoutSnapshotRow {
                        mode: LayoutMode::Single,
                        split_workspace_ids: None,
                        focused_workspace_id: Some(current.clone()),
                        captured_at: crate::domain::now_ms(),
                    },
                );
                if let Err(e) = save {
                    emit_json(
                        &conn,
                        &current,
                        "workspace.layout_snapshot_failed",
                        serde_json::json!({ "error": e.to_string() }),
                    );
                }
            }
        }
        close_open_rows(&conn, Some(&target.id))?;
        focus_and_sync(&conn, &target.id)?;
        self.view_of(&conn, &target.id)
    }

    /// Opens a (closed) workspace under the single-open constraint. Rejects
    /// with `AlreadyOpen` when another workspace is currently open — the
    /// sanctioned transition between open workspaces is `switch`.
    pub fn open_workspace(&self, workspace_id: &str) -> Result<WorkspaceView, RegistryV2Error> {
        let conn = self.conn()?;
        let entry = repo::find_by_id(&conn, workspace_id)?
            .ok_or_else(|| RegistryV2Error::NotFound(workspace_id.to_string()))?;
        if work_directories::list_by_workspace(&conn, workspace_id)?.is_empty() {
            return Err(RegistryV2Error::NoDirectories(workspace_id.to_string()));
        }
        open_single(&conn, &entry.id)?;
        self.view_of(&conn, &entry.id)
    }

    /// Closes a workspace (empty state) when it is open. Returns whether it
    /// was open. Idempotent. Does not touch the registry row or the disk.
    ///
    /// Without `force`, an open workspace with active runs
    /// (`running`/`queued`/`awaiting_approval`) refuses with
    /// [`RegistryV2Error::NeedConfirm`] — closing does not kill runs, but the
    /// user should know they will lose the surface watching them. Unsaved
    /// editor edits are guarded client-side (frontend ephemeral state) and
    /// are deliberately not checked here.
    pub fn close(&self, workspace_id: &str, force: bool) -> Result<bool, RegistryV2Error> {
        let conn = self.conn()?;
        let was_open = workspace_open_state::list(&conn)?
            .iter()
            .any(|r| r.workspace_id == workspace_id);
        if !was_open {
            return Ok(false);
        }
        if !force {
            let running = crate::store::repos::tasks_runs::list_active_runs_by_workspace(
                &conn,
                workspace_id,
            )?;
            if !running.is_empty() {
                return Err(RegistryV2Error::NeedConfirm {
                    workspace_id: workspace_id.to_string(),
                    running_tasks: running.into_iter().map(|r| r.id).collect(),
                });
            }
        }
        workspace_open_state::remove(&conn, workspace_id)?;
        if current_open_id(&conn)?.is_none() {
            clear_active(&conn)?;
        }
        emit(&conn, workspace_id, "workspace.closed");
        Ok(true)
    }

    /// Read-only close preview (ADR-0021 v2 close guard): the workspace's
    /// active runs with their task titles, in `created_at` order — what the
    /// close-confirm dialog shows. Zero side effects, safe to call any time.
    pub fn close_preview(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<RunningTaskSummary>, RegistryV2Error> {
        let conn = self.conn()?;
        let runs =
            crate::store::repos::tasks_runs::list_active_runs_by_workspace(&conn, workspace_id)?;
        let mut summaries = Vec::with_capacity(runs.len());
        for run in runs {
            let task_title = crate::store::repos::tasks_runs::get_task(&conn, &run.task_id)
                .ok()
                .map(|t| t.title)
                .unwrap_or_else(|| run.task_id.clone());
            summaries.push(RunningTaskSummary {
                run_id: run.id,
                task_title,
                status: run.status,
            });
        }
        Ok(summaries)
    }

    /// The currently open workspace with its directories and availability
    /// (design B5), or `None` in the empty state.
    pub fn get_open(&self) -> Result<Option<WorkspaceView>, RegistryV2Error> {
        let conn = self.conn()?;
        match current_open_id(&conn)? {
            Some(id) => Ok(Some(self.view_of(&conn, &id)?)),
            None => Ok(None),
        }
    }

    fn view_of(
        &self,
        conn: &Connection,
        workspace_id: &str,
    ) -> Result<WorkspaceView, RegistryV2Error> {
        let entry = repo::find_by_id(conn, workspace_id)?
            .ok_or_else(|| RegistryV2Error::NotFound(workspace_id.to_string()))?;
        let directories =
            work_directories::list_by_workspace_with_availability(conn, workspace_id)?;
        Ok(WorkspaceView { entry, directories })
    }
}

/// The single open workspace id: the focused row first, the most-recently
/// focused row as fallback for legacy multi-open leftovers.
fn current_open_id(conn: &Connection) -> Result<Option<String>, RegistryV2Error> {
    if let Some(focused) = workspace_open_state::find_focused(conn)? {
        return Ok(Some(focused.workspace_id));
    }
    Ok(workspace_open_state::list(conn)?
        .into_iter()
        .next()
        .map(|r| r.workspace_id))
}

/// Makes `workspace_id` the single open+focused workspace. Rejects when
/// another workspace is currently open — the sanctioned transition between
/// open workspaces is `switch`.
fn open_single(conn: &Connection, workspace_id: &str) -> Result<(), RegistryV2Error> {
    if let Some(other) = workspace_open_state::list(conn)?
        .into_iter()
        .find(|r| r.workspace_id != workspace_id)
    {
        return Err(RegistryV2Error::AlreadyOpen(other.workspace_id));
    }
    focus_and_sync(conn, workspace_id)
}

/// Closes every open row except `except` (single-open collapse: legitimate
/// current workspace plus possible legacy multi-open leftovers).
fn close_open_rows(conn: &Connection, except: Option<&str>) -> Result<(), RegistryV2Error> {
    for row in workspace_open_state::list(conn)? {
        if except == Some(row.workspace_id.as_str()) {
            continue;
        }
        if workspace_open_state::remove(conn, &row.workspace_id).is_ok() {
            emit(conn, &row.workspace_id, "workspace.closed");
        }
    }
    Ok(())
}

/// Inserts-or-refocuses the open row for `workspace_id` and keeps the
/// legacy `is_active` column and the recent list in sync.
fn focus_and_sync(conn: &Connection, workspace_id: &str) -> Result<(), RegistryV2Error> {
    let now = crate::domain::now_ms();
    let was_open = workspace_open_state::list(conn)?
        .iter()
        .any(|r| r.workspace_id == workspace_id);
    if !was_open {
        workspace_open_state::insert(
            conn,
            &WorkspaceOpenStateRow {
                workspace_id: workspace_id.to_string(),
                opened_at: now,
                last_focused_at: now,
                is_focused: true,
            },
        )?;
        emit(conn, workspace_id, "workspace.opened");
    }
    workspace_open_state::set_focused(conn, workspace_id)?;
    // Legacy is_active tracks the open workspace so the boot pass
    // (ensure_active_in_open_set) seeds the open set consistently.
    repo::set_active(conn, workspace_id)?;
    let pinned = repo::find_by_id(conn, workspace_id)?
        .map(|e| e.is_pinned)
        .unwrap_or(false);
    workspace_recent::touch(conn, workspace_id, now, pinned)?;
    emit(conn, workspace_id, "workspace.focused");
    Ok(())
}

/// Clears the legacy `is_active` flag (empty state — no open workspace).
fn clear_active(conn: &Connection) -> Result<(), RegistryV2Error> {
    conn.execute(
        "UPDATE workspaces SET is_active = 0 WHERE is_active = 1",
        [],
    )
    .map_err(StoreError::Sqlite)?;
    Ok(())
}

/// Derives the display name for a directory joining `workspace_id`, with
/// collision disambiguation against the workspace's existing names.
fn derive_display(
    conn: &Connection,
    workspace_id: &str,
    abs_path: &str,
) -> Result<String, RegistryV2Error> {
    let existing: Vec<String> = work_directories::list_by_workspace(conn, workspace_id)?
        .into_iter()
        .map(|d| d.display_name)
        .collect();
    workspace_label::derive_display_name(abs_path, &existing)
        .ok_or_else(|| RegistryV2Error::InvalidPath(abs_path.to_string()))
}

fn emit(conn: &Connection, workspace_id: &str, kind: &str) {
    let _ = events::append(
        conn,
        "workspace",
        workspace_id,
        kind,
        &serde_json::json!({}),
        crate::domain::now_ms(),
    );
}

fn emit_json(conn: &Connection, workspace_id: &str, kind: &str, payload: serde_json::Value) {
    let _ = events::append(
        conn,
        "workspace",
        workspace_id,
        kind,
        &payload,
        crate::domain::now_ms(),
    );
}

/// ADR-0021 `root_path` 兼容读取：v2 工作区的 `root_path` 列是 uuid 占位，
/// 真实根目录从 `work_directories` 派生（available 目录，`added_at` 序）；
/// 目录行为空或全部失效时回退 v1 语义（`root_path` 现场校验）。
/// 返回空 vec 表示该工作区当前无可达根目录。
pub fn compat_roots(conn: &Connection, ws: &WorkspaceEntry) -> Result<Vec<PathBuf>, StoreError> {
    let mut roots: Vec<PathBuf> =
        work_directories::list_by_workspace_with_availability(conn, &ws.id)?
            .iter()
            .filter(|d| d.available)
            .map(|d| PathBuf::from(&d.directory.abs_path))
            .collect();
    if roots.is_empty() {
        let legacy = Path::new(&ws.root_path);
        if legacy.is_dir() {
            roots.push(legacy.to_path_buf());
        }
    }
    Ok(roots)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn v2() -> (WorkspaceRegistryV2, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let reg = WorkspaceRegistryV2::new(dir.path().join("test.db"));
        (reg, dir)
    }

    fn make_dir(parent: &Path, name: &str) -> PathBuf {
        let p = parent.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn raw_conn(reg: &WorkspaceRegistryV2) -> Connection {
        let db = Db::open(&reg.db_path.to_string_lossy()).unwrap();
        migrations::run(&db.0).unwrap();
        db.0
    }

    fn dir_ids(view: &WorkspaceView) -> Vec<String> {
        view.directories
            .iter()
            .map(|s| s.directory.id.clone())
            .collect()
    }

    #[test]
    fn add_directory_from_empty_creates_and_opens_workspace() {
        let (reg, dir) = v2();
        let x = make_dir(dir.path(), "alpha");
        let view = reg.add_directory(&x).unwrap();
        assert_eq!(view.directories.len(), 1);
        assert_eq!(view.directories[0].directory.display_name, "alpha");
        assert!(view.directories[0].directory.mounted);
        assert!(view.directories[0].available);
        // root_path 为兼容占位（ADR-0021：不再写入有意义值）
        assert_eq!(view.entry.root_path, view.entry.id);
        // 打开态：单行 + 聚焦 + is_active 同步
        let open = reg.get_open().unwrap().unwrap();
        assert_eq!(open.entry.id, view.entry.id);
        let conn = raw_conn(&reg);
        let (open_rows, active): (i64, String) = conn
            .query_row(
                "SELECT (SELECT count(*) FROM workspace_open_state),
                        (SELECT id FROM workspaces WHERE is_active = 1)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(open_rows, 1);
        assert_eq!(active, view.entry.id);
    }

    #[test]
    fn add_directory_owned_by_closed_workspace_opens_it_without_theft() {
        let (reg, dir) = v2();
        let x = make_dir(dir.path(), "alpha");
        let first = reg.add_directory(&x).unwrap();
        reg.close(&first.entry.id, true).unwrap();
        assert!(reg.get_open().unwrap().is_none());

        // Re-opening the same directory reopens ITS workspace — no new one.
        let second = reg.add_directory(&x).unwrap();
        assert_eq!(second.entry.id, first.entry.id);
        assert_eq!(dir_ids(&second), dir_ids(&first));
    }

    #[test]
    fn add_directory_with_open_workspace_joins_and_steals() {
        let (reg, dir) = v2();
        let x = make_dir(dir.path(), "alpha");
        let y = make_dir(dir.path(), "beta");
        let z = make_dir(dir.path(), "gamma");
        let w1 = reg.add_directory(&x).unwrap();
        reg.add_directory(&y).unwrap();
        assert_eq!(w1.directories.len(), 1);

        // Close W1, create W2 from z, then re-add x: it must MOVE from the
        // closed W1 into the open W2 (globally-unique ownership).
        reg.close(&w1.entry.id, true).unwrap();
        let w2 = reg.add_directory(&z).unwrap();
        let moved = reg.add_directory(&x).unwrap();
        assert_eq!(moved.entry.id, w2.entry.id);
        assert_eq!(moved.directories.len(), 2);
        let conn = raw_conn(&reg);
        let w1_dirs: Vec<String> = conn
            .prepare("SELECT display_name FROM work_directories WHERE workspace_id = ?1")
            .unwrap()
            .query_map(rusqlite::params![w1.entry.id], |r| r.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(w1_dirs, vec!["beta".to_string()], "W1 keeps only beta");
    }

    #[test]
    fn add_directory_already_in_current_remounts() {
        let (reg, dir) = v2();
        let x = make_dir(dir.path(), "alpha");
        let view = reg.add_directory(&x).unwrap();
        let dir_id = view.directories[0].directory.id.clone();
        reg.unmount_directory(&view.entry.id, &dir_id).unwrap();
        let again = reg.add_directory(&x).unwrap();
        assert_eq!(again.directories.len(), 1, "no duplicate row");
        assert!(again.directories[0].directory.mounted, "re-mounted");
    }

    #[test]
    fn add_directory_rejects_missing_and_blacklisted() {
        let (reg, dir) = v2();
        let missing = dir.path().join("nope");
        assert!(matches!(
            reg.add_directory(&missing),
            Err(RegistryV2Error::DirectoryMissing(_))
        ));
        // 黑名单先行（对齐 v1 register 顺序），不依赖路径存在性
        let bad = Path::new("C:\\Windows");
        if bad.is_dir() || !bad.exists() {
            assert!(matches!(
                reg.add_directory(bad),
                Err(RegistryV2Error::Blacklisted(_))
            ));
        }
    }

    #[test]
    fn open_workspace_rejected_while_another_open() {
        let (reg, dir) = v2();
        let x = make_dir(dir.path(), "alpha");
        let z = make_dir(dir.path(), "gamma");
        let w1 = reg.add_directory(&x).unwrap();
        reg.close(&w1.entry.id, true).unwrap();
        let w2 = reg.add_directory(&z).unwrap();

        // 单开不变式：W2 打开时打开 W1 被拒
        let err = reg.open_workspace(&w1.entry.id).unwrap_err();
        assert!(matches!(err, RegistryV2Error::AlreadyOpen(id) if id == w2.entry.id));
        assert_eq!(reg.get_open().unwrap().unwrap().entry.id, w2.entry.id);
        // 幂等：打开已打开的工作区仅重新聚焦
        let again = reg.open_workspace(&w2.entry.id).unwrap();
        assert_eq!(again.entry.id, w2.entry.id);
        assert!(matches!(
            reg.open_workspace("ghost"),
            Err(RegistryV2Error::NotFound(_))
        ));
    }

    #[test]
    fn switch_saves_snapshot_closes_old_and_focuses_new() {
        let (reg, dir) = v2();
        let x = make_dir(dir.path(), "alpha");
        let z = make_dir(dir.path(), "gamma");
        let w1 = reg.add_directory(&x).unwrap();
        reg.close(&w1.entry.id, true).unwrap();
        let w2 = reg.add_directory(&z).unwrap();

        let switched = reg.switch(&w1.entry.id).unwrap();
        assert_eq!(switched.entry.id, w1.entry.id);
        // 快照记录被切走的工作区
        let conn = raw_conn(&reg);
        let snap = workspace_layout_snapshot::get(&conn).unwrap().unwrap();
        assert_eq!(
            snap.focused_workspace_id.as_deref(),
            Some(w2.entry.id.as_str())
        );
        assert_eq!(snap.mode, LayoutMode::Single);
        // 打开态单行聚焦 W1，is_active 同步
        let (open_id, active): (String, String) = conn
            .query_row(
                "SELECT (SELECT workspace_id FROM workspace_open_state WHERE is_focused = 1),
                        (SELECT id FROM workspaces WHERE is_active = 1)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(open_id, w1.entry.id);
        assert_eq!(active, w1.entry.id);
        // 切回 W2：快照更新为 W1
        reg.switch(&w2.entry.id).unwrap();
        let snap2 = workspace_layout_snapshot::get(&conn).unwrap().unwrap();
        assert_eq!(
            snap2.focused_workspace_id.as_deref(),
            Some(w1.entry.id.as_str())
        );
    }

    #[test]
    fn switch_with_blocked_snapshot_continues_with_warning() {
        let (reg, dir) = v2();
        let x = make_dir(dir.path(), "alpha");
        let z = make_dir(dir.path(), "gamma");
        let w1 = reg.add_directory(&x).unwrap();
        reg.close(&w1.entry.id, true).unwrap();
        let w2 = reg.add_directory(&z).unwrap();

        // 强制布局快照保存失败：BEFORE INSERT 触发器 RAISE
        {
            let conn = raw_conn(&reg);
            conn.execute_batch(
                "CREATE TRIGGER block_layout_save BEFORE INSERT ON workspace_layout_snapshot
                 BEGIN
                     SELECT RAISE(ABORT, 'blocked for test');
                 END;",
            )
            .unwrap();
        }

        let switched = reg.switch(&w1.entry.id).unwrap();
        assert_eq!(switched.entry.id, w1.entry.id, "切换不被快照失败阻塞");
        // 警告事件落库
        let conn = raw_conn(&reg);
        let warned: i64 = conn
            .query_row(
                "SELECT count(*) FROM events
                 WHERE aggregate_type = 'workspace' AND aggregate_id = ?1
                   AND kind = 'workspace.layout_snapshot_failed'",
                rusqlite::params![w2.entry.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(warned, 1);
        assert_eq!(reg.get_open().unwrap().unwrap().entry.id, w1.entry.id);
    }

    #[test]
    fn remove_last_directory_auto_closes() {
        let (reg, dir) = v2();
        let x = make_dir(dir.path(), "alpha");
        let w1 = reg.add_directory(&x).unwrap();
        let dir_id = w1.directories[0].directory.id.clone();

        let result = reg.remove_directory(&w1.entry.id, &dir_id).unwrap();
        assert!(result.workspace_closed);
        assert!(reg.get_open().unwrap().is_none());
        let conn = raw_conn(&reg);
        let open_rows: i64 = conn
            .query_row("SELECT count(*) FROM workspace_open_state", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(open_rows, 0);
        let active: i64 = conn
            .query_row(
                "SELECT count(*) FROM workspaces WHERE is_active = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(active, 0);
        // 注册行保留、磁盘文件不动
        let ws_rows: i64 = conn
            .query_row("SELECT count(*) FROM workspaces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ws_rows, 1);
        assert!(x.is_dir());
        // 空工作区不可再打开
        assert!(matches!(
            reg.open_workspace(&w1.entry.id),
            Err(RegistryV2Error::NoDirectories(_))
        ));
    }

    #[test]
    fn remove_directory_keeps_workspace_when_others_remain() {
        let (reg, dir) = v2();
        let x = make_dir(dir.path(), "alpha");
        let y = make_dir(dir.path(), "beta");
        let w1 = reg.add_directory(&x).unwrap();
        let after = reg.add_directory(&y).unwrap();
        let x_id = after.directories[0].directory.id.clone();

        let result = reg.remove_directory(&w1.entry.id, &x_id).unwrap();
        assert!(!result.workspace_closed);
        let open = reg.get_open().unwrap().unwrap();
        assert_eq!(open.entry.id, w1.entry.id);
        assert_eq!(open.directories.len(), 1);
        assert_eq!(open.directories[0].directory.display_name, "beta");
    }

    #[test]
    fn remove_directory_wrong_workspace_is_not_found() {
        let (reg, dir) = v2();
        let x = make_dir(dir.path(), "alpha");
        let z = make_dir(dir.path(), "gamma");
        let w1 = reg.add_directory(&x).unwrap();
        reg.close(&w1.entry.id, true).unwrap();
        let w2 = reg.add_directory(&z).unwrap();
        let w1_dir = w1.directories[0].directory.id.clone();
        assert!(matches!(
            reg.remove_directory(&w2.entry.id, &w1_dir),
            Err(RegistryV2Error::DirectoryNotFound(_))
        ));
    }

    #[test]
    fn mount_unmount_roundtrip() {
        let (reg, dir) = v2();
        let x = make_dir(dir.path(), "alpha");
        let w1 = reg.add_directory(&x).unwrap();
        let dir_id = w1.directories[0].directory.id.clone();
        reg.unmount_directory(&w1.entry.id, &dir_id).unwrap();
        let view = reg.get_open().unwrap().unwrap();
        assert!(!view.directories[0].directory.mounted);
        reg.mount_directory(&w1.entry.id, &dir_id).unwrap();
        let view = reg.get_open().unwrap().unwrap();
        assert!(view.directories[0].directory.mounted);
        assert!(matches!(
            reg.mount_directory("ghost", &dir_id),
            Err(RegistryV2Error::DirectoryNotFound(_))
        ));
    }

    /// Seeds one active run bound to the workspace (mirrors the shell's
    /// insert_task + insert_run + bind_run_workspace write path).
    fn seed_active_run(reg: &WorkspaceRegistryV2, ws_id: &str, title: &str) -> String {
        use crate::domain::{Run, RunState, Task, TaskStatus};
        let conn = raw_conn(reg);
        let now = 1000i64;
        let task = Task {
            id: format!("task-{title}"),
            session_id: None,
            title: title.into(),
            description: String::new(),
            status: TaskStatus::Running,
            created_at: now,
            updated_at: now,
            workspace_id: ws_id.into(),
        };
        crate::store::repos::tasks_runs::insert_task(&conn, &task).unwrap();
        let run = Run {
            id: format!("run-{title}"),
            task_id: task.id.clone(),
            session_id: "sess".into(),
            status: RunState::Running,
            heartbeat_at: now,
            created_at: now,
            updated_at: now,
        };
        crate::store::repos::tasks_runs::insert_run(&conn, &run).unwrap();
        crate::store::repos::tasks_runs::bind_run_workspace(&conn, &run.id, ws_id).unwrap();
        run.id
    }

    #[test]
    fn close_refuses_with_running_tasks_unless_forced() {
        let (reg, dir) = v2();
        let x = make_dir(dir.path(), "alpha");
        let w1 = reg.add_directory(&x).unwrap();
        let run_id = seed_active_run(&reg, &w1.entry.id, "deploy");

        // Non-forced close refuses and reports the active run.
        match reg.close(&w1.entry.id, false) {
            Err(RegistryV2Error::NeedConfirm {
                workspace_id,
                running_tasks,
            }) => {
                assert_eq!(workspace_id, w1.entry.id);
                assert_eq!(run_id, *running_tasks.first().unwrap());
            }
            other => panic!("expected NeedConfirm, got {other:?}"),
        }
        // The workspace is still open after the refusal.
        assert!(reg.get_open().unwrap().is_some());

        // Forced close goes through.
        assert!(reg.close(&w1.entry.id, true).unwrap());
        assert!(reg.get_open().unwrap().is_none());
    }

    #[test]
    fn close_preview_lists_active_runs_with_task_titles() {
        let (reg, dir) = v2();
        let x = make_dir(dir.path(), "alpha");
        let w1 = reg.add_directory(&x).unwrap();
        seed_active_run(&reg, &w1.entry.id, "deploy");

        let preview = reg.close_preview(&w1.entry.id).unwrap();
        assert_eq!(preview.len(), 1);
        assert_eq!(preview[0].run_id, "run-deploy");
        assert_eq!(preview[0].task_title, "deploy");
        assert_eq!(
            preview[0].status,
            crate::domain::run_state::RunState::Running
        );

        // A workspace without active runs previews empty.
        let (reg2, dir2) = v2();
        let y = make_dir(dir2.path(), "beta");
        let w2 = reg2.add_directory(&y).unwrap();
        assert!(reg2.close_preview(&w2.entry.id).unwrap().is_empty());
        // Preview never closes anything — the workspace stays open.
        assert!(reg2.get_open().unwrap().is_some());
    }
}
