//! Shared application state for command handlers.
//!
//! Commands stay thin: they validate arguments and call into nuomi-core
//! services/facade. This struct bundles what they need.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nuomi_core::facade::{NuomiConfig, NuomiKernel, ProviderSource};
use nuomi_core::providers::{OsKeyring, SecretStore};
use nuomi_core::services::{
    workspace_layout::WorkspaceLayoutService, workspace_migration,
    workspace_registry::WorkspaceRegistry, GitService, WorkspaceService,
};
use nuomi_core::{CoreError, CoreResult};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

/// Tracks in-flight background team runs so a board transition to
/// `cancelled` can stop their executors. `run_id → (task_id, token)`.
#[derive(Clone, Default)]
pub struct RunCancelRegistry {
    entries: Arc<Mutex<HashMap<String, CancelEntry>>>,
}

type CancelEntry = (String, Arc<CancellationToken>);

impl RunCancelRegistry {
    pub(crate) fn register(&self, run_id: &str, task_id: &str, token: CancellationToken) {
        self.entries
            .lock()
            .expect("cancel registry poisoned")
            .insert(run_id.to_string(), (task_id.to_string(), Arc::new(token)));
    }

    pub(crate) fn remove(&self, run_id: &str) {
        self.entries
            .lock()
            .expect("cancel registry poisoned")
            .remove(run_id);
    }

    /// Cancels every active run of `task_id`; returns the affected run ids.
    pub(crate) fn cancel_by_task(&self, task_id: &str) -> Vec<String> {
        let mut map = self.entries.lock().expect("cancel registry poisoned");
        let hits: Vec<String> = map
            .iter()
            .filter(|(_, (tid, _))| tid == task_id)
            .map(|(rid, _)| rid.clone())
            .collect();
        for rid in &hits {
            if let Some((_, token)) = map.get(rid) {
                token.cancel();
            }
            map.remove(rid);
        }
        hits
    }

    /// Cancels the executor of a single run; returns whether a live token
    /// was found (false = the run was already settled or never supervised).
    pub(crate) fn cancel_by_run(&self, run_id: &str) -> bool {
        let mut map = self.entries.lock().expect("cancel registry poisoned");
        match map.remove(run_id) {
            Some((_, token)) => {
                token.cancel();
                true
            }
            None => false,
        }
    }
}

/// Tracks in-flight conversation runs (`session_id → token`) so `/stop`
/// and the tray can abort a chat run that has no `runs` row of its own.
#[derive(Clone, Default)]
pub struct SessionCancelRegistry {
    tokens: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl SessionCancelRegistry {
    /// Starts a new supervised run for `session_id`, cancelling a previous
    /// one (only one live run per conversation is meaningful).
    pub(crate) fn begin(&self, session_id: &str) -> CancellationToken {
        let token = CancellationToken::new();
        let previous = self
            .tokens
            .lock()
            .expect("session cancel registry poisoned")
            .insert(session_id.to_string(), token.clone());
        if let Some(prev) = previous {
            prev.cancel();
        }
        token
    }

    /// Signals the running conversation loop to stop at its next step
    /// boundary; returns whether one was in flight.
    pub(crate) fn cancel(&self, session_id: &str) -> bool {
        match self
            .tokens
            .lock()
            .expect("session cancel registry poisoned")
            .remove(session_id)
        {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    /// Deregisters a finished run. A no-op when the run was cancelled
    /// (the canceller already removed it).
    pub(crate) fn finish(&self, session_id: &str) {
        self.tokens
            .lock()
            .expect("session cancel registry poisoned")
            .remove(session_id);
    }
}

/// Everything a command handler needs, testable without a Tauri runtime.
#[derive(Clone)]
pub struct AppState {
    /// Shared so background run dispatchers can outlive a command call.
    pub kernel: std::sync::Arc<NuomiKernel>,
    /// Database path (approval gate / task/run repos open short-lived
    /// connections via spawn_blocking).
    pub db_path: Arc<str>,
    /// Sandboxed filesystem root exposed to the UI. Swappable at runtime
    /// (worktree switch): services re-point without a kernel restart.
    pub workspace_root: std::sync::Arc<std::sync::RwLock<PathBuf>>,
    /// Whether the workspace root was pinned via `NUOMI_WORKSPACE_ROOT` at
    /// boot. When set, the UI treats the workspace as configured even if the
    /// persisted settings row is absent (the env always wins).
    pub workspace_env_configured: bool,
    /// Workspace registry service handle. Constructed from `db_path` at boot;
    /// each call opens a fresh connection (cheap to clone — just a PathBuf).
    pub workspace_registry: WorkspaceRegistry,
    /// Secret store backing provider keyring references. OS keyring in prod;
    /// tests/demo inject an in-memory store.
    pub secrets: Arc<dyn SecretStore>,
    /// Cancellation tokens for supervised background team runs.
    pub run_cancels: RunCancelRegistry,
    /// Cancellation tokens for conversation runs (chat has no `runs` row).
    pub session_cancels: SessionCancelRegistry,
    /// Generation counter bumped by every integration upsert/delete; the
    /// notifier dispatcher watches this to hot-reload its sinks without an
    /// app restart.
    pub integrations_reload_tx: watch::Sender<u64>,
    /// Receiver half of [`AppState::integrations_reload_tx`], cloned into
    /// the notifier loop at spawn time.
    pub integrations_reload: watch::Receiver<u64>,
}

impl AppState {
    /// Boots with the OS keyring as secret store (production default).
    pub async fn boot(
        db_path: PathBuf,
        provider: ProviderSource,
        default_workspace: PathBuf,
    ) -> CoreResult<Self> {
        Self::boot_with_secrets(db_path, provider, Arc::new(OsKeyring), default_workspace).await
    }

    /// Boots the kernel with an explicit provider source and secret store
    /// (tests/demo pass a [`nuomi_core::providers::MemorySecretStore`]) and
    /// resolves the workspace root from `NUOMI_WORKSPACE_ROOT` or falls back
    /// to `<default_workspace>`.
    ///
    /// The default workspace root MUST NOT be the process cwd: under
    /// `tauri dev` that is `src-tauri/`, so saving a watched source file from
    /// the built-in editor would make the dev watcher rebuild and kill the
    /// app ("flash quit"); in a packaged build the cwd is an arbitrary system
    /// directory. Callers pass the app's sandbox dir instead.
    pub async fn boot_with_secrets(
        db_path: PathBuf,
        provider: ProviderSource,
        secrets: Arc<dyn SecretStore>,
        default_workspace: PathBuf,
    ) -> CoreResult<Self> {
        let boot_started = std::time::Instant::now();
        // Phase 1: kernel boot (SQLite open + migrations + first session +
        // plugin registration). The only real IO on this path.
        let kernel = NuomiKernel::boot(NuomiConfig {
            db_path: db_path.clone(),
            provider,
            // Plugin side-load dirs default to env + user config (ADR 0009);
            // the desktop shell adds no extra paths of its own.
            plugin_paths: Vec::new(),
        })
        .await?;
        let kernel_boot_ms = boot_started.elapsed().as_millis() as u64;
        let db_path: Arc<str> = Arc::from(db_path.to_string_lossy().to_string());
        // Phase 2: legacy single-workspace migration. Runs after kernel boot
        // (which applied migrations 0016/0017) and before workspace resolution.
        // Corrects the placeholder row from 0016 with a real uuid-v7 id,
        // reparents `__migrated__` sessions and migrates codebase-memory.
        let migration_started = std::time::Instant::now();
        {
            let path = db_path.clone();
            tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
                let db = nuomi_core::store::Db::open(&path)?;
                nuomi_core::store::migrations::run(&db.0)?;
                let _ = workspace_migration::run_if_needed(&db.0)?;
                Ok(())
            })
            .await
            .map_err(join_err)??;
        }
        let migration_ms = migration_started.elapsed().as_millis() as u64;
        // Workspace priority: `NUOMI_WORKSPACE_ROOT` env > active registry row
        // > persisted setting (migration 0008 `app_settings`) > caller default.
        // The env flag marks the workspace as configured regardless of persistence.
        let env_root = std::env::var_os("NUOMI_WORKSPACE_ROOT").map(PathBuf::from);
        let workspace_env_configured = env_root.is_some();
        // Phase 3: resolve workspace root. Read the active registry row first;
        // fall back to env > persisted setting > caller default. Deliberately
        // after kernel boot + migration: the `workspaces` table only exists
        // once migrations 0016/0017 ran.
        let settings_started = std::time::Instant::now();
        let registry = WorkspaceRegistry::new(PathBuf::from(db_path.to_string()));
        let active_from_registry = {
            let reg = registry.clone();
            tokio::task::spawn_blocking(move || reg.current_active())
                .await
                .map_err(join_err)??
        };
        let persisted = {
            let path = db_path.clone();
            tokio::task::spawn_blocking(move || -> Result<Option<String>, CoreError> {
                let db = nuomi_core::store::Db::open(&path)?;
                Ok(nuomi_core::store::repos::settings::get(
                    &db.0,
                    nuomi_core::store::repos::settings::WORKSPACE_ROOT,
                )?)
            })
            .await
            .map_err(join_err)??
        };
        let workspace_resolve_ms = settings_started.elapsed().as_millis() as u64;
        let workspace_root = env_root
            .or_else(|| active_from_registry.map(|e| PathBuf::from(e.root_path)))
            .or_else(|| persisted.map(PathBuf::from))
            .unwrap_or(default_workspace);
        // The resolved root must exist: team-run CLI members spawn with it as
        // their cwd, and a missing dir fails the spawn (os error 267).
        std::fs::create_dir_all(&workspace_root)
            .map_err(|e| CoreError::Store(nuomi_core::store::StoreError::Io(e)))?;
        // Phase 4: restore multi-workspace layout snapshot (open set + focus +
        // split). Pinned workspaces are always restored; non-pinned are restored
        // per app_settings (default true). Directory-missing workspaces are
        // silently skipped — restore never blocks startup.
        {
            let path = db_path.clone();
            let _ = tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
                let db = nuomi_core::store::Db::open(&path)?;
                nuomi_core::store::migrations::run(&db.0)?;
                let restore_non_pinned = nuomi_core::store::repos::settings::get(
                    &db.0,
                    nuomi_core::store::repos::settings::RESTORE_NON_PINNED_ON_STARTUP,
                )?
                .map(|v| v != "false")
                .unwrap_or(true);
                let svc = WorkspaceLayoutService::new(PathBuf::from(path.to_string()));
                let _ = svc.restore_snapshot(restore_non_pinned);
                Ok(())
            })
            .await;
        }
        let (integrations_reload_tx, integrations_reload) = watch::channel(0u64);
        tracing::info!(
            kernel_boot_ms,
            migration_ms,
            workspace_resolve_ms,
            total_ms = boot_started.elapsed().as_millis() as u64,
            "AppState boot complete"
        );
        Ok(Self {
            kernel: Arc::new(kernel),
            db_path,
            workspace_root: std::sync::Arc::new(std::sync::RwLock::new(workspace_root)),
            workspace_env_configured,
            workspace_registry: registry,
            secrets,
            run_cancels: RunCancelRegistry::default(),
            session_cancels: SessionCancelRegistry::default(),
            integrations_reload_tx,
            integrations_reload,
        })
    }

    /// Current sandbox root (cloned to avoid holding the lock).
    pub fn current_workspace(&self) -> PathBuf {
        self.workspace_root
            .read()
            .expect("workspace lock poisoned")
            .clone()
    }

    /// Validates and switches the sandbox root (must be an existing dir).
    /// Best-effort syncs the registry active state: if the path matches a
    /// registered workspace, its `is_active` flag is transferred. Unregistered
    /// paths (legacy / env override) skip the registry update.
    pub fn switch_workspace(&self, path: PathBuf) -> CoreResult<PathBuf> {
        if !path.is_dir() {
            return Err(CoreError::Workspace(
                nuomi_core::services::WorkspaceError::InvalidPath(
                    path.to_string_lossy().to_string(),
                ),
            ));
        }
        // Best-effort: activate the matching registry row. Failures (path not
        // registered, db locked) are logged but do not block the switch — the
        // in-memory root is the source of truth for the running process.
        if let Ok(Some(entry)) = self.workspace_registry.find_by_path(&path) {
            if let Err(e) = self.workspace_registry.activate(&entry.id) {
                tracing::warn!(error = %e, "registry activate on switch failed");
            }
        }
        let mut root = self
            .workspace_root
            .write()
            .expect("workspace lock poisoned");
        let previous = std::mem::replace(&mut *root, path);
        Ok(previous)
    }

    pub fn workspace(&self) -> CoreResult<WorkspaceService> {
        Ok(WorkspaceService::new(self.current_workspace())?)
    }

    pub fn git(&self) -> GitService {
        GitService::new(self.current_workspace())
    }

    /// Resolves a `WorkspaceService` for an explicit workspace id (ADR 0017).
    /// Returns the active workspace service when `workspace_id` is `None`
    /// (backward-compatible with pre-ADR-0017 callers). Returns
    /// `WorkspaceError::InvalidPath` when the id is unknown or its root
    /// directory is missing.
    pub fn workspace_for(&self, workspace_id: Option<&str>) -> CoreResult<WorkspaceService> {
        match workspace_id {
            Some(id) => {
                let entry = self
                    .workspace_registry
                    .find_by_id(id)
                    .map_err(|e| CoreError::Workspace(nuomi_core::services::WorkspaceError::InvalidPath(e.to_string())))?
                    .ok_or_else(|| {
                        CoreError::Workspace(nuomi_core::services::WorkspaceError::InvalidPath(
                            id.to_string(),
                        ))
                    })?;
                Ok(WorkspaceService::new(PathBuf::from(entry.root_path))?)
            }
            None => self.workspace(),
        }
    }

    /// Resolves a `GitService` for an explicit workspace id (ADR 0017).
    pub fn git_for(&self, workspace_id: Option<&str>) -> CoreResult<GitService> {
        match workspace_id {
            Some(id) => {
                let entry = self
                    .workspace_registry
                    .find_by_id(id)
                    .map_err(|e| CoreError::Workspace(nuomi_core::services::WorkspaceError::InvalidPath(e.to_string())))?
                    .ok_or_else(|| {
                        CoreError::Workspace(nuomi_core::services::WorkspaceError::InvalidPath(
                            id.to_string(),
                        ))
                    })?;
                Ok(GitService::new(PathBuf::from(entry.root_path)))
            }
            None => Ok(self.git()),
        }
    }
}

/// Maps a cancelled/panicked `spawn_blocking` task into a store-backed
/// `CoreError` (same shape as `facade::join_err`, kept local on purpose).
pub(crate) fn join_err(e: tokio::task::JoinError) -> CoreError {
    CoreError::Store(nuomi_core::store::StoreError::Sqlite(
        rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
    ))
}
