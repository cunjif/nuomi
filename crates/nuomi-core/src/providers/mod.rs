//! Provider clients (OpenAICompatible / AnthropicCompatible) and master-slave
//! orchestration.

pub mod anthropic;
pub mod client;
pub mod context;
pub mod fake;
pub mod master;
pub mod openai;
pub mod pool;
pub mod secrets;
pub mod sse;
pub mod types;

pub use anthropic::AnthropicCompatibleClient;
pub use client::LlmProvider;
pub use fake::FakeLlm;
pub use master::{MasterSlaveRouter, SlaveAsTool};
pub use openai::OpenAiCompatibleClient;
pub use pool::{shared_client, warm, warm_from_store, WarmResult};
pub use secrets::{MemorySecretStore, OsKeyring, SecretStore};
pub use types::{
    ChatMessage, ChatRequest, ChatResponse, MessageRole, StreamEvent, ToolCall, ToolDef, Usage,
};

use thiserror::Error;

/// Errors produced by the provider layer.
#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("provider '{0}' not configured")]
    NotConfigured(String),

    #[error("no provider matches required capabilities: {0}")]
    NoMatchingCapability(String),

    #[error("all providers in fallback chain failed: {0}")]
    AllFallbacksFailed(String),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("protocol violation from '{provider}': {message}")]
    Protocol {
        provider: &'static str,
        message: String,
    },

    #[error("keyring error: {0}")]
    Keyring(String),

    /// Error surfaced by an external CLI agent adapter (`adapters::cli`).
    #[error("cli agent '{agent}': {message}")]
    CliAgent { agent: String, message: String },
}
