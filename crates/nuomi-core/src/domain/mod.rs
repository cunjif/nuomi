//! Domain entities and the run/session state machines (single source of truth).

pub mod boot_health;
pub mod entities;
pub mod evolution_run_state;
pub mod run_state;
pub mod status;
pub mod steward;
pub mod steward_enums;

pub use boot_health::{
    app_version, clear_attempt, health_check, read_attempt, record_attempt, should_safe_mode,
    BootAttempt, HealthReport, CRASH_THRESHOLD,
};
pub use entities::{
    AgentProfile, AgentRefKind, Approval, ApprovalDecision, Attachment, AttachmentKind,
    AutoRemember, B1Config, Capability, CliFlavor, ConversationKind, ConversationParticipant,
    EventRecord, EventRollback, EvolutionSettings, Integration, IntegrationKind, MemoryEntry,
    MemoryPolicy, MessageQueueEntry, ModelEntry, OnlineLearningConfig, PromptStatus, PromptVersion,
    Proposal, ProposalStatus, ProviderConfig, ProviderProtocol, QueueStatus, RefineConfig,
    RefineStrategy, RetrievalStrategy, Role, RoleMemoryConfig, RouteMode, Run, Schedule,
    ScheduleSessionMode, ScheduleTargetKind, Session, SessionCliHandle, SkillCreationConfig,
    SkillFormat, Task, TaskStatus, Team, TeamTopology, TodoItem, WhiteBoardNote,
    WhiteboardRouteMode,
};
pub use evolution_run_state::{transition as evolution_transition, EvolutionRunState};
pub use run_state::{
    ApprovalOutcome, GenerationalRun, LandingError, LandingEvent, LandingPhase, LandingRecord,
    LandingTracker, LandingTransitionError, MutationError, RunEvent, RunState, TransitionError,
};
pub use status::{derive_status, DerivedStatus, RunFacts};
pub use steward::{
    DevRoleBinding, DevTeam, EvolutionArtifact, EvolutionCycle, EvolutionDataPool, EvolutionTask,
    GateDecision, StewardAi, StewardChangeSnapshot, StewardSession,
};
pub use steward_enums::{
    ArtifactStatus, ArtifactType, CyclePhase, CycleStatus, DevRoleKind, GateDecisionKind,
    TaskPhase, TaskStatus as StewardTaskStatus, TriggerSource,
};

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
