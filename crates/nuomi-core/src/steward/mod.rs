//! Steward AI — the application-level meta-intelligence.
//!
//! The steward acts as the "butler" for the app itself, distinct from
//! RoleAgents that execute user tasks. It manages configuration, evolution
//! scheduling, data cleansing, and an internal dev team for self-evolution.
//!
//! Module layout (design.md §2.4):
//! - `kernel`: session management + message handling (K-Steward-2)
//! - `snapshot`: read-only app-state aggregation (K-Steward-2)
//! - `intent`: intent recognition trait + LLM impl (K-Steward-2)
//! - `config_change`: configuration proposals + rollback (K-Steward-3)
//! - `dev_team`: dev team singleton + role bindings (K-Steward-4)
//! - `task`: evolution task dependency graph (K-Steward-4)
//! - `cleanse`: data cleansing pipeline (K-Steward-5)
//! - `cycle`: evolution cycle orchestrator (K-Steward-6)
//! - `gate`: evolution gate + artifact merger (K-Steward-7)
//! - `artifact`: artifact repository + diff (K-Steward-7)
//! - `events`: steward.* event types + logger (K-Steward-8)
//! - `tools`: LLM-callable tool registry for the steward agent loop
//! - `llm_loop`: LLM-driven agent loop (conversation + tool orchestration + suspend protocol)
//! - `attachments`: steward-scoped attachment intake (whitelist + cap + content-addressed storage)
//! - `context_builder`: attachment context assembly (text/pdf/office/images, design §2.1.3.2)
//! - `context_usage`: read-only usage aggregation for the indicator/popup
//! - `prompt_optimizer`: one-shot draft rewrite on the bound provider

pub mod artifact;
pub mod attachments;
pub mod cleanse;
pub mod config_change;
pub mod context_builder;
pub mod context_usage;
pub mod cycle;
pub mod dev_team;
pub mod events;
pub mod gate;
pub mod intent;
pub mod kernel;
pub mod llm_loop;
pub mod prompt_optimizer;
pub mod settings;
pub mod snapshot;
pub mod task;
pub mod tools;

use thiserror::Error;

/// Errors produced by the steward module.
#[derive(Debug, Error)]
pub enum StewardError {
    #[error("not found: {entity}#{id}")]
    NotFound { entity: &'static str, id: String },

    #[error("attachment {name} unusable: {reason}")]
    AttachmentInvalid { name: String, reason: String },

    #[error("conflict on {entity}#{id}: {reason}")]
    Conflict {
        entity: &'static str,
        id: String,
        reason: String,
    },

    #[error("not authorized: {0}")]
    NotAuthorized(String),

    #[error("dev team not ready: {0}")]
    DevTeamNotReady(String),

    #[error("store error: {0}")]
    Store(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("intent recognition failed: {0}")]
    Intent(String),
}

impl From<crate::store::StoreError> for StewardError {
    fn from(e: crate::store::StoreError) -> Self {
        StewardError::Store(e.to_string())
    }
}

impl From<rusqlite::Error> for StewardError {
    fn from(e: rusqlite::Error) -> Self {
        StewardError::Store(format!("sqlite: {e}"))
    }
}

impl From<serde_json::Error> for StewardError {
    fn from(e: serde_json::Error) -> Self {
        StewardError::Store(format!("serialization: {e}"))
    }
}
