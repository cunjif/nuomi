//! Shared application state for command handlers.
//!
//! Commands stay thin: they validate arguments and call into nuomi-core
//! services/facade. This struct bundles what they need.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nuomi_core::facade::{NuomiConfig, NuomiKernel, ProviderSource};
use nuomi_core::providers::{OsKeyring, SecretStore};
use nuomi_core::services::{GitService, WorkspaceService};
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
}

/// Everything a command handler needs, testable without a Tauri runtime.
pub struct AppState {
    /// Shared so background run dispatchers can outlive a command call.
    pub kernel: std::sync::Arc<NuomiKernel>,
    /// Database path (approval gate / task/run repos open short-lived
    /// connections via spawn_blocking).
    pub db_path: Arc<str>,
    /// Sandboxed filesystem root exposed to the UI. Swappable at runtime
    /// (worktree switch): services re-point without a kernel restart.
    pub workspace_root: std::sync::RwLock<PathBuf>,
    /// Whether the workspace root was pinned via `NUOMI_WORKSPACE_ROOT` at
    /// boot. When set, the UI treats the workspace as configured even if the
    /// persisted settings row is absent (the env always wins).
    pub workspace_env_configured: bool,
    /// Secret store backing provider keyring references. OS keyring in prod;
    /// tests/demo inject an in-memory store.
    pub secrets: Arc<dyn SecretStore>,
    /// Cancellation tokens for supervised background team runs.
    pub run_cancels: RunCancelRegistry,
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
        })
        .await?;
        let kernel_boot_ms = boot_started.elapsed().as_millis() as u64;
        let db_path: Arc<str> = Arc::from(db_path.to_string_lossy().to_string());
        // Workspace priority: `NUOMI_WORKSPACE_ROOT` env > persisted setting
        // (migration 0008 `app_settings`) > the caller's default. The env
        // flag marks the workspace as configured regardless of persistence.
        let env_root = std::env::var_os("NUOMI_WORKSPACE_ROOT").map(PathBuf::from);
        let workspace_env_configured = env_root.is_some();
        // Phase 2: persisted workspace root read. Deliberately kept AFTER
        // kernel boot: the `app_settings` table only exists once migrations
        // ran, so this read depends on phase 1 (parallelizing would race the
        // first-boot migration for no measurable gain — the read itself is a
        // single indexed lookup on a freshly opened connection).
        let settings_started = std::time::Instant::now();
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
            .or_else(|| persisted.map(PathBuf::from))
            .unwrap_or(default_workspace);
        // The resolved root must exist: team-run CLI members spawn with it as
        // their cwd, and a missing dir fails the spawn (os error 267).
        std::fs::create_dir_all(&workspace_root)
            .map_err(|e| CoreError::Store(nuomi_core::store::StoreError::Io(e)))?;
        let (integrations_reload_tx, integrations_reload) = watch::channel(0u64);
        tracing::info!(
            kernel_boot_ms,
            workspace_resolve_ms,
            total_ms = boot_started.elapsed().as_millis() as u64,
            "AppState boot complete"
        );
        Ok(Self {
            kernel: Arc::new(kernel),
            db_path,
            workspace_root: std::sync::RwLock::new(workspace_root),
            workspace_env_configured,
            secrets,
            run_cancels: RunCancelRegistry::default(),
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
    pub fn switch_workspace(&self, path: PathBuf) -> CoreResult<PathBuf> {
        if !path.is_dir() {
            return Err(CoreError::Workspace(
                nuomi_core::services::WorkspaceError::InvalidPath(
                    path.to_string_lossy().to_string(),
                ),
            ));
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
}

/// Maps a cancelled/panicked `spawn_blocking` task into a store-backed
/// `CoreError` (same shape as `facade::join_err`, kept local on purpose).
fn join_err(e: tokio::task::JoinError) -> CoreError {
    CoreError::Store(nuomi_core::store::StoreError::Sqlite(
        rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
    ))
}
