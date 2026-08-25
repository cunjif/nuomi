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
    /// Secret store backing provider keyring references. OS keyring in prod;
    /// tests/demo inject an in-memory store.
    pub secrets: Arc<dyn SecretStore>,
    /// Cancellation tokens for supervised background team runs.
    pub run_cancels: RunCancelRegistry,
}

impl AppState {
    /// Boots with the OS keyring as secret store (production default).
    pub async fn boot(db_path: PathBuf, provider: ProviderSource) -> CoreResult<Self> {
        Self::boot_with_secrets(db_path, provider, Arc::new(OsKeyring)).await
    }

    /// Boots the kernel with an explicit provider source and secret store
    /// (tests/demo pass a [`nuomi_core::providers::MemorySecretStore`]) and
    /// resolves the workspace root from `NUOMI_WORKSPACE_ROOT` or falls back
    /// to the current directory.
    pub async fn boot_with_secrets(
        db_path: PathBuf,
        provider: ProviderSource,
        secrets: Arc<dyn SecretStore>,
    ) -> CoreResult<Self> {
        let kernel = NuomiKernel::boot(NuomiConfig {
            db_path: db_path.clone(),
            provider,
        })
        .await?;
        let db_path: Arc<str> = Arc::from(db_path.to_string_lossy().to_string());
        let workspace_root = std::env::var_os("NUOMI_WORKSPACE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        Ok(Self {
            kernel: Arc::new(kernel),
            db_path,
            workspace_root: std::sync::RwLock::new(workspace_root),
            secrets,
            run_cancels: RunCancelRegistry::default(),
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
