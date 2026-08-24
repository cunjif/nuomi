//! Role/Team executors: pipeline, router, group chat (Selector + Handoff), whiteboard.
//! Milestone K6 / SPEC T9.

pub mod group_chat;
pub mod input;
pub mod pipeline;
pub mod router;
pub mod selector;
pub mod whiteboard;

pub use group_chat::{GroupChatExecutor, GroupChatOutcome, HANDOFF_TOOL};
pub use input::{ProviderResolver, TeamRunInput};
pub use pipeline::{PipelineExecutor, PipelineOutcome, PipelineStep};
pub use router::RouterExecutor;
pub use selector::{
    GroupMember, GroupState, GroupTurn, HeuristicSelector, LlmSelector, RoundRobinSelector,
    SpeakerSelector,
};
pub use whiteboard::WhiteBoardService;

use thiserror::Error;

/// Errors produced by the orchestrator layer.
#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("team '{team}' member '{member}' not found")]
    MemberNotFound { team: String, member: String },

    #[error("no agent matches required capabilities: {0}")]
    NoMatchingAgent(String),

    #[error("handoff loop detected: {chain} exceeds max hops {max}")]
    HandoffLoopDetected { chain: String, max: u32 },

    #[error("group chat exceeded max rounds ({0}) without convergence")]
    MaxRoundsExceeded(u32),

    #[error("provider failure: {0}")]
    Provider(String),

    #[error("store failure: {0}")]
    Store(String),

    #[error("invalid team config: {0}")]
    InvalidTeam(String),
}
