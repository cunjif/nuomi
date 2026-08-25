//! Shared application state for command handlers.
//!
//! Commands stay thin: they validate arguments and call into nuomi-core
//! services/facade. This struct bundles what they need.

use std::path::PathBuf;
use std::sync::Arc;

use nuomi_core::facade::{NuomiConfig, NuomiKernel, ProviderSource};
use nuomi_core::services::{GitService, WorkspaceService};
use nuomi_core::{CoreError, CoreResult};

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
}

impl AppState {
    /// Boots the kernel with an explicit provider source (endpoint in prod,
    /// fake in tests/demo) and resolves the workspace root from `NUOMI_WORKSPACE_ROOT`
    /// or falls back to the current directory.
    pub async fn boot(db_path: PathBuf, provider: ProviderSource) -> CoreResult<Self> {
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
