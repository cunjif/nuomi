//! LSP client kernel (ADR-0027): Language Server Protocol integration.
//!
//! Provides a lightweight LSP client that manages Language Server processes
//! per language, communicates over stdio using JSON-RPC 2.0, and exposes
//! a request/response + notification API for the IPC layer.
//!
//! Architecture:
//! - `transport`: stdio read/write + LSP message framing (Content-Length headers)
//! - `types`: configuration types (LspServerConfig, LspLanguageId)
//! - `client`: LspClient — JSON-RPC 2.0 over stdio, request/response/notification
//! - `manager`: LspManager — multi-language LS registry + process lifecycle
//! - `defaults`: per-language default LS command mapping
//! - `detector`: PATH + SDK auto-detection (ADR-0028)

pub mod client;
pub mod defaults;
pub mod detector;
pub mod manager;
pub mod transport;
pub mod types;

pub use client::LspClient;
pub use manager::{ConfigSource, LspManager};
pub use types::{LspLanguageId, LspServerConfig, LspServerStatus};

use thiserror::Error;

/// Errors produced by the LSP client kernel.
#[derive(Debug, Error)]
pub enum LspError {
    #[error("language server for '{0}' not started")]
    NotStarted(String),

    #[error("language server for '{0}' already started")]
    AlreadyStarted(String),

    #[error("language server process failed to start: {0}")]
    ProcessSpawn(String),

    #[error("language server process exited unexpectedly (code={0:?})")]
    ProcessExited(Option<i32>),

    #[error("JSON-RPC transport error: {0}")]
    Transport(String),

    #[error("JSON-RPC protocol error: {0}")]
    Protocol(String),

    #[error("language server request timed out after {0:?}")]
    Timeout(std::time::Duration),

    #[error("language server returned error: code={code} message={message}")]
    ServerError { code: i32, message: String },

    #[error("language server for '{0}' is not available (not installed or misconfigured)")]
    NotAvailable(String),

    #[error("initialize handshake failed: {0}")]
    InitializeFailed(String),
}
