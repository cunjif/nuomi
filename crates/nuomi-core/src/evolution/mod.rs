//! Self-evolution: trajectory aggregation, GEPA-inspired reflection,
//! prompt versioning and allowlisted online research (SPEC T11, AC12–AC14).

pub mod journal;
pub mod reflection;
pub mod research;
pub mod review;
pub mod scheduler;
pub mod trajectory;
pub mod versioning;

pub use journal::{
    audit, DriftConfig, DriftDetector, EvolutionJournal, JournalEntry, JournalFilter, JournalKind,
    JournaledReviewGate, JOURNAL_KIND_PREFIX, JOURNAL_SINK_SESSION_ID,
};
pub use reflection::{PromptCandidate, ReflectionInput, Reflector};
pub use research::{
    online_authorized, set_online_authorized, ResearchAllowlist, ResearchFetcher,
    ResearchReportEntry, ResearchScheduler,
};
pub use review::{DefaultReviewGate, ReviewGate, ReviewProposal, ReviewVerdict};
pub use scheduler::{CooldownDecision, CooldownGate, PeriodicResearch, DEFAULT_COOLDOWN};
pub use trajectory::{TrajectoryAggregator, TrajectorySummary};
pub use versioning::{ApplyBaseline, ApplySnapshot, PromptVersionManager};

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

    #[error(
        "baseline conflict on '{plugin}': planning baseline was {expected_id} \
         but the current active version is {actual_id}"
    )]
    BaselineConflict {
        plugin: String,
        expected_id: String,
        actual_id: String,
    },

    #[error("nothing to roll back: the applied version had no prior active prompt")]
    NothingToRollBack,

    #[error("journal entry #{0} is not an Applied entry")]
    JournalNotApplied(u64),

    #[error("journal data corruption: {0}")]
    JournalCorrupt(String),

    #[error("journal io error: {0}")]
    JournalIo(#[from] std::io::Error),
}
