//! Self-evolution: trajectory aggregation, GEPA-inspired reflection,
//! prompt versioning and allowlisted online research (SPEC T11, AC12–AC14).

pub mod reflection;
pub mod research;
pub mod scheduler;
pub mod trajectory;
pub mod versioning;

pub use reflection::{PromptCandidate, ReflectionInput, Reflector};
pub use research::{
    online_authorized, set_online_authorized, ResearchAllowlist, ResearchFetcher,
    ResearchReportEntry, ResearchScheduler,
};
pub use scheduler::PeriodicResearch;
pub use trajectory::{TrajectoryAggregator, TrajectorySummary};
pub use versioning::PromptVersionManager;

use thiserror::Error;

/// Errors produced by the evolution engine.
#[derive(Debug, Error)]
pub enum EvolutionError {
    #[error("no trajectories available for reflection")]
    NoTrajectories,

    #[error("research source '{0}' is not on the allowlist")]
    SourceNotAllowlisted(String),

    #[error("online learning is not authorized")]
    NotAuthorized,

    #[error("prompt version conflict on '{plugin}' at v{version}")]
    VersionConflict { plugin: String, version: i64 },

    #[error("store failure: {0}")]
    Store(String),

    #[error("provider failure during reflection: {0}")]
    Provider(String),

    #[error("reflection output is not valid JSON with 'content'/'diff': {0}")]
    InvalidReflectionOutput(String),

    #[error("research fetch failed for '{url}': {message}")]
    FetchFailed { url: String, message: String },
}
