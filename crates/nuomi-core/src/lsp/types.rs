//! LSP configuration and type definitions (ADR-0027).

use serde::{Deserialize, Serialize};

/// Language identifier matching LSP `LanguageId` and Monaco language IDs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LspLanguageId(pub String);

impl LspLanguageId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for LspLanguageId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for LspLanguageId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl std::fmt::Display for LspLanguageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Configuration for a single Language Server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspServerConfig {
    /// Language this server handles (e.g. "python", "java", "go").
    pub language: LspLanguageId,
    /// Command to launch the Language Server (e.g. "pyright-langserver").
    pub command: String,
    /// Arguments passed to the command (e.g. ["--stdio"]).
    pub args: Vec<String>,
    /// Whether this server is enabled by the user.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Environment variables to set when launching the process.
    #[serde(default)]
    pub env: Vec<(String, String)>,
    /// Working directory for the Language Server process.
    #[serde(default)]
    pub working_dir: Option<String>,
    /// Timeout for the `initialize` handshake in seconds (default 30).
    #[serde(default = "default_init_timeout")]
    pub init_timeout_secs: u64,
}

fn default_true() -> bool {
    true
}

fn default_init_timeout() -> u64 {
    30
}

impl LspServerConfig {
    pub fn new(language: impl Into<LspLanguageId>, command: impl Into<String>) -> Self {
        Self {
            language: language.into(),
            command: command.into(),
            args: Vec::new(),
            enabled: true,
            env: Vec::new(),
            working_dir: None,
            init_timeout_secs: default_init_timeout(),
        }
    }

    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Runtime status of a Language Server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LspServerStatus {
    /// Not yet started (lazy: will start on first file open for this language).
    NotStarted,
    /// Process spawned, waiting for `initialize` response.
    Initializing,
    /// `initialize` succeeded, server is ready for requests.
    Ready,
    /// Server process exited or failed to start.
    Failed(String),
    /// User disabled this language server.
    Disabled,
}

impl LspServerStatus {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

impl std::fmt::Display for LspServerStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotStarted => f.write_str("not_started"),
            Self::Initializing => f.write_str("initializing"),
            Self::Ready => f.write_str("ready"),
            Self::Failed(msg) => write!(f, "failed: {msg}"),
            Self::Disabled => f.write_str("disabled"),
        }
    }
}
