//! Top-level error type for nuomi-core.

use thiserror::Error;

/// Unified library error. Module-specific errors convert via `From`.
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("store error: {0}")]
    Store(#[from] crate::store::StoreError),

    #[error("domain error: {0}")]
    Domain(#[from] crate::domain::DomainError),

    #[error("harness error: {0}")]
    Harness(#[from] crate::harness::HarnessError),

    #[error("provider error: {0}")]
    Provider(#[from] crate::providers::ProviderError),

    #[error("orchestrator error: {0}")]
    Orchestrator(#[from] crate::orchestrator::OrchestratorError),

    #[error("evolution error: {0}")]
    Evolution(#[from] crate::evolution::EvolutionError),
}

/// Convenience result alias used across the crate.
pub type CoreResult<T> = Result<T, CoreError>;
