//! External agent adapters (SPEC: docs/specs/cli-agents-m1.md).
//!
//! Adapters wrap non-HTTP agents behind the kernel-facing
//! [`crate::providers::LlmProvider`] trait so they can join Teams as
//! first-class members with zero orchestrator changes. The v1 adapter is
//! `cli`: external CLI agent processes (Claude Code / Codex / plain-text
//! scripts) spawned safely — arg arrays only, executable allowlist,
//! `kill_on_drop` reaping.

pub mod cli;
pub mod pty;
pub mod traits;

pub use cli::CliAgentClient;
pub use pty::{
    strip_ansi, DeliveryReadiness, PromptAnchor, PromptDetector, PromptPattern, PtySession,
    ReadinessPoller, PROMPT_PATTERNS,
};
pub use traits::{
    AdapterCapabilities, AgentAdapter, BinaryResolver, EnvSanitizer, ResumableSession,
};

use thiserror::Error;

/// Errors produced by the adapters layer.
#[derive(Debug, Error)]
pub enum AdapterError {
    /// The profile's executable is not on the caller-supplied allowlist.
    #[error("command '{command}' is not allowlisted for cli agents")]
    CommandNotAllowlisted { command: String },

    /// The child process failed to start.
    #[error("failed to spawn '{command}': {source}")]
    Spawn {
        command: String,
        source: std::io::Error,
    },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The agent violated its flavor protocol (or config was malformed).
    #[error("protocol violation from '{agent}': {message}")]
    Protocol { agent: String, message: String },
}
