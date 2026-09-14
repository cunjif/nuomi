//! Structured IPC error: stable `code` strings the frontend maps to i18n.

use serde::Serialize;

use nuomi_core::{providers::ProviderError, store::StoreError, CoreError};

/// Outward-facing error for every command.
#[derive(Debug, Serialize, specta::Type, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub enum IpcError {
    #[error("{message}")]
    Generic {
        code: &'static str,
        message: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        details: Option<serde_json::Value>,
    },
}

impl IpcError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self::Generic {
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(
        code: &'static str,
        message: impl Into<String>,
        details: serde_json::Value,
    ) -> Self {
        Self::Generic {
            code,
            message: message.into(),
            details: Some(details),
        }
    }
}

impl From<CoreError> for IpcError {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::Store(StoreError::NotFound { entity, id }) => {
                Self::new("store.not_found", format!("{entity}#{id} not found"))
            }
            CoreError::Store(StoreError::Conflict { .. }) => {
                Self::new("store.conflict", "stale state, retry")
            }
            CoreError::Store(other) => Self::new("store.failed", other.to_string()),
            CoreError::Domain(err) => Self::new("domain.invalid", err.to_string()),
            CoreError::Harness(err) => Self::new("harness.failed", err.to_string()),
            CoreError::Provider(ProviderError::NoMatchingCapability(caps)) => {
                Self::new("provider.no_match", format!("no provider matches: {caps}"))
            }
            CoreError::Provider(ProviderError::AllFallbacksFailed(msg)) => {
                Self::new("provider.all_failed", msg)
            }
            CoreError::Provider(err) => Self::new("provider.failed", err.to_string()),
            CoreError::Orchestrator(err) => Self::new("orchestrator.failed", err.to_string()),
            CoreError::Evolution(err) => Self::new("evolution.failed", err.to_string()),
            CoreError::Workspace(err) => map_workspace(&err),
            CoreError::WorkspaceRegistry(err) => map_registry_error(&err),
            CoreError::WorkspaceMigration(err) => map_migration_error(&err),
            CoreError::Git(err) => map_git(&err),
            CoreError::Scheduler(err) => Self::new("scheduler.failed", err.to_string()),
        }
    }
}

fn map_workspace(err: &nuomi_core::services::workspace::WorkspaceError) -> IpcError {
    use nuomi_core::services::workspace::WorkspaceError as E;
    match err {
        E::Escape { .. } => IpcError::new("workspace.escape_denied", err.to_string()),
        E::InvalidPath(_) | E::NotADirectory(_) => {
            IpcError::new("workspace.invalid_path", err.to_string())
        }
        E::Io(_) => IpcError::new("workspace.io", err.to_string()),
    }
}

fn map_registry_error(err: &nuomi_core::services::RegistryError) -> IpcError {
    use nuomi_core::services::RegistryError as E;
    match err {
        E::InvalidPath(_) => IpcError::new("workspace.invalid_path", err.to_string()),
        E::Blacklisted(_) => IpcError::new("workspace.escape_denied", err.to_string()),
        E::AlreadyExists(_) => IpcError::new("workspace.already_exists", err.to_string()),
        E::NotFound(_) => IpcError::new("workspace.not_found", err.to_string()),
        E::DirectoryMissing(_) => IpcError::new("workspace.directory_missing", err.to_string()),
        E::Io(_) => IpcError::new("workspace.io", err.to_string()),
        E::Store(_) => IpcError::new("store.failed", err.to_string()),
    }
}

fn map_migration_error(err: &nuomi_core::services::MigrationOrchestrationError) -> IpcError {
    use nuomi_core::services::MigrationOrchestrationError as E;
    match err {
        E::Store(_) => IpcError::new("store.failed", err.to_string()),
        E::Io(_) => IpcError::new("workspace.io", err.to_string()),
    }
}

fn map_git(err: &nuomi_core::services::git_service::GitError) -> IpcError {
    use nuomi_core::services::git_service::GitError as E;
    match err {
        E::NotARepository(path) => IpcError::new(
            "git.not_a_repository",
            format!("not a git repository: {path}"),
        ),
        E::SubcommandNotAllowed(cmd) => IpcError::new(
            "git.command_not_allowed",
            format!("git {cmd} is not allowed"),
        ),
        E::CommandFailed { cmd, stderr } => IpcError::with_details(
            "git.command_failed",
            format!("git {cmd} failed"),
            serde_json::json!({ "command": cmd, "stderr": stderr }),
        ),
        E::Io(e) => IpcError::new("git.failed", e.to_string()),
    }
}

impl From<ProviderError> for IpcError {
    fn from(e: ProviderError) -> Self {
        CoreError::from(e).into()
    }
}

impl From<StoreError> for IpcError {
    fn from(e: StoreError) -> Self {
        CoreError::from(e).into()
    }
}

impl From<rusqlite::Error> for IpcError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sqlite(e).into()
    }
}

impl From<nuomi_core::services::WorkspaceError> for IpcError {
    fn from(e: nuomi_core::services::WorkspaceError) -> Self {
        CoreError::from(e).into()
    }
}

impl From<nuomi_core::services::RegistryError> for IpcError {
    fn from(e: nuomi_core::services::RegistryError) -> Self {
        CoreError::from(e).into()
    }
}

impl From<nuomi_core::services::MigrationOrchestrationError> for IpcError {
    fn from(e: nuomi_core::services::MigrationOrchestrationError) -> Self {
        CoreError::from(e).into()
    }
}

impl From<nuomi_core::services::GitError> for IpcError {
    fn from(e: nuomi_core::services::GitError) -> Self {
        CoreError::from(e).into()
    }
}

impl From<tokio::task::JoinError> for IpcError {
    fn from(e: tokio::task::JoinError) -> Self {
        Self::new("core.join_failed", e.to_string())
    }
}

/// Stable code constants (frontend i18n keys reference these).
pub mod codes {
    pub const STORE_NOT_FOUND: &str = "store.not_found";
    pub const STORE_CONFLICT: &str = "store.conflict";
    pub const DOMAIN_INVALID: &str = "domain.invalid";
    pub const HARNESS_FAILED: &str = "harness.failed";
    pub const PROVIDER_NO_MATCH: &str = "provider.no_match";
    pub const PROVIDER_ALL_FAILED: &str = "provider.all_failed";
    pub const PROVIDER_FAILED: &str = "provider.failed";
    pub const ORCHESTRATOR_FAILED: &str = "orchestrator.failed";
    pub const EVOLUTION_FAILED: &str = "evolution.failed";
    pub const WORKSPACE_ESCAPE_DENIED: &str = "workspace.escape_denied";
    pub const WORKSPACE_ALREADY_EXISTS: &str = "workspace.already_exists";
    pub const WORKSPACE_NOT_FOUND: &str = "workspace.not_found";
    pub const WORKSPACE_DIRECTORY_MISSING: &str = "workspace.directory_missing";
    pub const WORKSPACE_IO: &str = "workspace.io";
    pub const GIT_COMMAND_NOT_ALLOWED: &str = "git.command_not_allowed";
    pub const GIT_NOT_A_REPOSITORY: &str = "git.not_a_repository";
    pub const SCHEDULER_FAILED: &str = "scheduler.failed";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_core_errors_to_stable_codes() {
        let cases: Vec<(CoreError, &'static str)> = vec![
            (
                CoreError::Store(StoreError::NotFound {
                    entity: "run",
                    id: "r1".into(),
                }),
                codes::STORE_NOT_FOUND,
            ),
            (
                CoreError::Store(StoreError::Conflict {
                    entity: "run",
                    id: "r1".into(),
                    expected: "queued".to_string(),
                }),
                codes::STORE_CONFLICT,
            ),
        ];
        for (err, expected) in cases {
            let ipc: IpcError = err.into();
            let code = match &ipc {
                IpcError::Generic { code, .. } => *code,
            };
            assert_eq!(code, expected);
        }
    }
}
