//! Domain entities and the run/session state machines (single source of truth).

pub mod entities;
pub mod run_state;
pub mod status;

pub use entities::{
    AgentProfile, AgentRefKind, Approval, ApprovalDecision, Attachment, AttachmentKind,
    Capability, CliFlavor, ConversationKind, ConversationParticipant, EventRecord, Integration,
    IntegrationKind, MemoryEntry, ModelEntry, PromptStatus, PromptVersion, ProviderConfig,
    ProviderProtocol, Role, RouteMode, Run, Schedule, ScheduleSessionMode, ScheduleTargetKind,
    Session, Task, TaskStatus, Team, TeamTopology, TodoItem, WhiteBoardNote, WhiteboardRouteMode,
};
pub use run_state::{
    ApprovalOutcome, GenerationalRun, LandingError, LandingEvent, LandingPhase, LandingRecord,
    LandingTracker, LandingTransitionError, MutationError, RunEvent, RunState, TransitionError,
};
pub use status::{derive_status, DerivedStatus, RunFacts};

use thiserror::Error;

/// Errors produced by domain rules.
#[derive(Debug, Error)]
pub enum DomainError {
    #[error("invalid transition: {from} --{event:?}-->")]
    InvalidTransition { from: String, event: String },
    #[error("invalid entity: {0}")]
    InvalidEntity(String),
}

/// Generates a new uuid-v7 string id (project-wide convention).
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Current unix time in milliseconds (project-wide convention: `*At` fields).
pub fn now_ms() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}
