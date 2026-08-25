//! Domain entities and the run/session state machines (single source of truth).

pub mod entities;
pub mod run_state;

pub use entities::{
    AgentProfile, Approval, ApprovalDecision, CliFlavor, EventRecord, Integration, IntegrationKind,
    MemoryEntry, PromptStatus, PromptVersion, ProviderConfig, ProviderProtocol, Role, Run,
    Schedule, Session, Task, TaskStatus, Team, TeamTopology, WhiteBoardNote,
};
pub use run_state::{ApprovalOutcome, RunEvent, RunState, TransitionError};

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
