//! Workspace registry domain service: orchestrates registration, activation,
//! removal and listing by composing the repo, path guard, `.nuomi` layout,
//! codebase-memory migrator and color palette.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use thiserror::Error;
use uuid::Uuid;

use super::codebase_memory_migrator;
use super::nuomi_dir;
use super::workspace_guard;
use super::workspace_palette;
use crate::store::repos::events;
use crate::store::repos::workspaces as repo;
use crate::store::repos::workspaces::WorkspaceEntry;
use crate::store::{migrations, Db, StoreError};

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("path does not exist or is not a directory: {0}")]
    InvalidPath(String),
    #[error("path is blacklisted: {0}")]
    Blacklisted(String),
    #[error("workspace already exists: {0}")]
    AlreadyExists(String),
    #[error("workspace not found: {0}")]
    NotFound(String),
    #[error("workspace directory missing or unreadable: {0}")]
    DirectoryMissing(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
}

impl From<codebase_memory_migrator::MigrationError> for RegistryError {
    fn from(e: codebase_memory_migrator::MigrationError) -> Self {
        match e {
            codebase_memory_migrator::MigrationError::Io(io) => RegistryError::Io(io),
        }
    }
}

impl From<nuomi_dir::NuomiDirError> for RegistryError {
    fn from(e: nuomi_dir::NuomiDirError) -> Self {
        match e {
            nuomi_dir::NuomiDirError::Conflict(s) => RegistryError::Io(std::io::Error::other(s)),
            nuomi_dir::NuomiDirError::Io(io) => RegistryError::Io(io),
        }
    }
}

/// Result of removing a workspace (activation may transfer or go empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoveResult {
    pub removed_id: String,
    pub new_active_id: Option<String>,
}

/// A workspace entry with a runtime-probed directory presence flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEntryWithPresence {
    pub entry: WorkspaceEntry,
    pub directory_present: bool,
}

/// The registry service. Holds the db path; each call opens a fresh connection
/// (matching the `commands.rs` pattern for blocking store access).
#[derive(Clone)]
pub struct WorkspaceRegistry {
    db_path: PathBuf,
}

impl WorkspaceRegistry {
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    fn conn(&self) -> Result<Connection, RegistryError> {
        let db = Db::open(&self.db_path.to_string_lossy())?;
        migrations::run(&db.0)?;
        Ok(db.0)
    }

    /// Registers `path` as a new workspace.
    /// Order: canonicalize → blacklist → uniqueness → ensure .nuomi →
    ///        migrate codebase-memory → color → insert → activate if first.
    pub fn register(&self, path: &Path) -> Result<WorkspaceEntry, RegistryError> {
        if workspace_guard::is_blacklisted(path) {
            return Err(RegistryError::Blacklisted(
                path.to_string_lossy().to_string(),
            ));
        }
        let canonical = workspace_guard::canonicalize(path).map_err(|e| match e {
            workspace_guard::PathGuardError::InvalidPath(p) => RegistryError::InvalidPath(p),
            workspace_guard::PathGuardError::Blacklisted(p) => RegistryError::Blacklisted(p),
            workspace_guard::PathGuardError::Io(io) => RegistryError::Io(io),
        })?;
        if workspace_guard::is_blacklisted(&canonical) {
            return Err(RegistryError::Blacklisted(
                canonical.to_string_lossy().to_string(),
            ));
        }
        let root_str = canonical.to_string_lossy().to_string();
        let conn = self.conn()?;
        if repo::find_by_path(&conn, &root_str)?.is_some() {
            return Err(RegistryError::AlreadyExists(root_str));
        }
        // `.nuomi/` + codebase-memory migration (filesystem side-effects).
        nuomi_dir::ensure_nuomi_dir(&canonical)?;
        let _ = codebase_memory_migrator::migrate(&canonical)?;
        let was_empty = repo::list(&conn)?.is_empty();
        let mut entry = WorkspaceEntry {
            id: Uuid::now_v7().to_string(),
            root_path: root_str,
            color_tag: workspace_palette::color_for(&canonical.to_string_lossy())
                .0
                .to_string(),
            created_at: crate::domain::now_ms(),
            is_active: false,
            is_pinned: false,
        };
        repo::insert(&conn, &entry)?;
        if was_empty {
            repo::set_active(&conn, &entry.id)?;
            entry.is_active = true;
            // Reparent sessions that still carry the '__migrated__' sentinel
            // (created before any workspace was registered) into this first
            // workspace so they don't become permanent orphans.
            conn.execute(
                "UPDATE sessions SET workspace_id = ?1 WHERE workspace_id = '__migrated__'",
                rusqlite::params![entry.id],
            )
            .map_err(StoreError::Sqlite)?;
            // Also add to open set + focus (multi-workspace model).
            let now = crate::domain::now_ms();
            let _ = crate::store::repos::workspace_open_state::insert(
                &conn,
                &crate::store::repos::workspace_open_state::WorkspaceOpenStateRow {
                    workspace_id: entry.id.clone(),
                    opened_at: now,
                    last_focused_at: now,
                    is_focused: true,
                },
            );
            let _ = crate::store::repos::workspace_recent::touch(&conn, &entry.id, now, false);
            emit_workspace_event(&conn, &entry.id, "workspace.opened");
            emit_workspace_event(&conn, &entry.id, "workspace.focused");
        }
        Ok(entry)
    }

    /// Removes workspace `id`. If it was active, activation transfers to the
    /// first remaining entry (by created_at); empty registry → None.
    /// Never touches the filesystem or session data.
    pub fn remove(&self, id: &str) -> Result<RemoveResult, RegistryError> {
        let conn = self.conn()?;
        let existing =
            repo::find_by_id(&conn, id)?.ok_or_else(|| RegistryError::NotFound(id.to_string()))?;
        let was_active = existing.is_active;
        repo::remove(&conn, id)?;
        let new_active_id = if was_active {
            let remaining = repo::list(&conn)?;
            if let Some(first) = remaining.first() {
                repo::set_active(&conn, &first.id)?;
                Some(first.id.clone())
            } else {
                None
            }
        } else {
            None
        };
        Ok(RemoveResult {
            removed_id: id.to_string(),
            new_active_id,
        })
    }

    /// Activates workspace `id`. Validates the root directory is readable
    /// before transferring activation; on failure the active state is unchanged.
    pub fn activate(&self, id: &str) -> Result<WorkspaceEntry, RegistryError> {
        let conn = self.conn()?;
        let entry =
            repo::find_by_id(&conn, id)?.ok_or_else(|| RegistryError::NotFound(id.to_string()))?;
        let root = Path::new(&entry.root_path);
        if !root.is_dir() {
            return Err(RegistryError::DirectoryMissing(entry.root_path.clone()));
        }
        repo::set_active(&conn, id)?;
        Ok(WorkspaceEntry {
            is_active: true,
            ..entry
        })
    }

    /// Lists all workspaces with a runtime directory-presence probe.
    pub fn list(&self) -> Result<Vec<WorkspaceEntryWithPresence>, RegistryError> {
        let conn = self.conn()?;
        let entries = repo::list(&conn)?;
        Ok(entries
            .into_iter()
            .map(|entry| {
                let directory_present = Path::new(&entry.root_path).is_dir();
                WorkspaceEntryWithPresence {
                    entry,
                    directory_present,
                }
            })
            .collect())
    }

    /// Returns the currently active workspace, if any.
    pub fn current_active(&self) -> Result<Option<WorkspaceEntry>, RegistryError> {
        let conn = self.conn()?;
        Ok(repo::find_active(&conn)?)
    }

    /// Finds a workspace by id.
    pub fn find_by_id(&self, id: &str) -> Result<Option<WorkspaceEntry>, RegistryError> {
        let conn = self.conn()?;
        Ok(repo::find_by_id(&conn, id)?)
    }

    /// Finds a workspace by (canonicalized) path.
    pub fn find_by_path(&self, path: &Path) -> Result<Option<WorkspaceEntry>, RegistryError> {
        let conn = self.conn()?;
        let canonical = match workspace_guard::canonicalize(path) {
            Ok(c) => c,
            Err(workspace_guard::PathGuardError::InvalidPath(p)) => {
                return Err(RegistryError::InvalidPath(p));
            }
            Err(workspace_guard::PathGuardError::Blacklisted(p)) => {
                return Err(RegistryError::Blacklisted(p));
            }
            Err(workspace_guard::PathGuardError::Io(io)) => return Err(RegistryError::Io(io)),
        };
        Ok(repo::find_by_path(&conn, &canonical.to_string_lossy())?)
    }

    /// Pins a workspace (marks it as always-restored on startup).
    pub fn pin(&self, id: &str) -> Result<(), RegistryError> {
        let conn = self.conn()?;
        repo::pin(&conn, id)?;
        emit_workspace_event(&conn, id, "workspace.pinned");
        Ok(())
    }

    /// Unpins a workspace.
    pub fn unpin(&self, id: &str) -> Result<(), RegistryError> {
        let conn = self.conn()?;
        repo::unpin(&conn, id)?;
        emit_workspace_event(&conn, id, "workspace.unpinned");
        Ok(())
    }
}

fn emit_workspace_event(conn: &Connection, workspace_id: &str, kind: &str) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn registry() -> (WorkspaceRegistry, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let reg = WorkspaceRegistry::new(db_path);
        (reg, dir)
    }

    fn make_ws_dir(parent: &Path, name: &str) -> PathBuf {
        let p = parent.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn register_first_workspace_auto_activates() {
        let (reg, dir) = registry();
        let ws = make_ws_dir(dir.path(), "project-a");
        let entry = reg.register(&ws).unwrap();
        assert!(entry.is_active);
        let active = reg.current_active().unwrap().unwrap();
        assert_eq!(active.id, entry.id);
    }

    #[test]
    fn register_second_workspace_does_not_steal_active() {
        let (reg, dir) = registry();
        let a = make_ws_dir(dir.path(), "a");
        let b = make_ws_dir(dir.path(), "b");
        let ea = reg.register(&a).unwrap();
        let eb = reg.register(&b).unwrap();
        assert!(ea.is_active);
        assert!(!eb.is_active);
        // Active stays on first.
        let active = reg.current_active().unwrap().unwrap();
        assert_eq!(active.id, ea.id);
    }

    #[test]
    fn register_duplicate_path_returns_already_exists() {
        let (reg, dir) = registry();
        let ws = make_ws_dir(dir.path(), "dup");
        reg.register(&ws).unwrap();
        let err = reg.register(&ws).unwrap_err();
        assert!(matches!(err, RegistryError::AlreadyExists(_)));
    }

    #[test]
    fn register_creates_nuomi_dir() {
        let (reg, dir) = registry();
        let ws = make_ws_dir(dir.path(), "ws");
        reg.register(&ws).unwrap();
        assert!(ws.join(".nuomi").is_dir());
    }

    #[test]
    fn register_assigns_color_from_palette() {
        let (reg, dir) = registry();
        let ws = make_ws_dir(dir.path(), "colored");
        let entry = reg.register(&ws).unwrap();
        assert!(
            workspace_palette::PALETTE
                .iter()
                .any(|c| c.0 == entry.color_tag),
            "color must be from palette"
        );
    }

    #[test]
    fn activate_transfers_and_returns_entry() {
        let (reg, dir) = registry();
        let a = make_ws_dir(dir.path(), "a");
        let b = make_ws_dir(dir.path(), "b");
        let ea = reg.register(&a).unwrap();
        let eb = reg.register(&b).unwrap();
        let activated = reg.activate(&eb.id).unwrap();
        assert_eq!(activated.id, eb.id);
        assert!(activated.is_active);
        let active = reg.current_active().unwrap().unwrap();
        assert_eq!(active.id, eb.id);
        assert_ne!(active.id, ea.id);
    }

    #[test]
    fn activate_missing_directory_returns_directory_missing() {
        let (reg, dir) = registry();
        let ws = make_ws_dir(dir.path(), "ghost");
        let entry = reg.register(&ws).unwrap();
        // Remove the directory after registration.
        fs::remove_dir_all(&ws).unwrap();
        let err = reg.activate(&entry.id).unwrap_err();
        assert!(matches!(err, RegistryError::DirectoryMissing(_)));
        // Active state unchanged (still active from registration).
        let active = reg.current_active().unwrap().unwrap();
        assert_eq!(active.id, entry.id);
    }

    #[test]
    fn remove_active_transfers_to_first_remaining() {
        let (reg, dir) = registry();
        let a = make_ws_dir(dir.path(), "a");
        let b = make_ws_dir(dir.path(), "b");
        let ea = reg.register(&a).unwrap();
        let eb = reg.register(&b).unwrap();
        // Activate b, then remove b → activation transfers to a (first by created_at).
        reg.activate(&eb.id).unwrap();
        let result = reg.remove(&eb.id).unwrap();
        assert_eq!(result.removed_id, eb.id);
        assert_eq!(result.new_active_id, Some(ea.id.clone()));
        let active = reg.current_active().unwrap().unwrap();
        assert_eq!(active.id, ea.id);
    }

    #[test]
    fn remove_last_workspace_enters_empty_state() {
        let (reg, dir) = registry();
        let ws = make_ws_dir(dir.path(), "solo");
        let entry = reg.register(&ws).unwrap();
        let result = reg.remove(&entry.id).unwrap();
        assert_eq!(result.new_active_id, None);
        assert!(reg.current_active().unwrap().is_none());
        assert!(reg.list().unwrap().is_empty());
    }

    #[test]
    fn remove_preserves_nuomi_dir_and_sessions() {
        let (reg, dir) = registry();
        let ws = make_ws_dir(dir.path(), "keep");
        let entry = reg.register(&ws).unwrap();
        reg.remove(&entry.id).unwrap();
        // .nuomi/ is NOT deleted.
        assert!(ws.join(".nuomi").is_dir());
    }

    #[test]
    fn list_reports_directory_presence() {
        let (reg, dir) = registry();
        let ws = make_ws_dir(dir.path(), "present");
        let entry = reg.register(&ws).unwrap();
        let list = reg.list().unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].directory_present);
        // Delete the dir → presence flips to false.
        fs::remove_dir_all(&ws).unwrap();
        let list2 = reg.list().unwrap();
        assert!(!list2[0].directory_present);
        let _ = entry;
    }

    #[test]
    fn remove_nonexistent_returns_not_found() {
        let (reg, _dir) = registry();
        let err = reg.remove("nope").unwrap_err();
        assert!(matches!(err, RegistryError::NotFound(_)));
    }

    #[test]
    fn activate_nonexistent_returns_not_found() {
        let (reg, _dir) = registry();
        let err = reg.activate("nope").unwrap_err();
        assert!(matches!(err, RegistryError::NotFound(_)));
    }
}
