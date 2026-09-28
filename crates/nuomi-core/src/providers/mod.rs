//! Provider clients (OpenAICompatible / AnthropicCompatible) and master-slave
//! orchestration.

pub mod anthropic;
pub mod catalog;
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
pub use catalog::list_model_ids;
pub use client::LlmProvider;
pub use fake::FakeLlm;
pub use master::{MasterSlaveRouter, SlaveAsTool};
pub use openai::OpenAiCompatibleClient;
pub use pool::{client_for_endpoint, shared_client, warm, warm_from_store, WarmResult};
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

    /// Non-2xx HTTP response. Unlike `Http`, this captures the response body
    /// so the caller can see the provider's error payload (e.g. invalid model
    /// name, expired key, rate-limit details) instead of just the status code.
    #[error("http status {status} from {url}: {body}")]
    HttpStatus {
        status: reqwest::StatusCode,
        url: String,
        body: String,
    },

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

/// Joins a provider `base_url` with a versioned API path `suffix`, tolerating
/// both bare origins (`https://host`) and origins already carrying a version
/// prefix (`https://host/v1`).
///
/// `suffix` is the path *after* the canonical `/v1` segment — e.g.
/// `/chat/completions` (OpenAI-compatible) or `/messages` (Anthropic). A bare
/// origin (no path beyond the authority) gets `/v1` prepended; any origin that
/// already carries a path keeps `suffix` appended as-is, so `/v1` is never
/// doubled and custom prefixes (`/api/v1`, `/v2`, …) are preserved.
///
/// Trailing slashes on `base_url` are trimmed. An empty `base_url` yields the
/// bare `suffix`; callers that need to reject emptiness do so before calling.
pub(crate) fn join_api_path(base_url: &str, suffix: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return suffix.to_string();
    }
    // Everything after the `scheme://` authority: if it contains a `/` the
    // caller already supplied a path (version or custom prefix); otherwise it
    // is a bare origin and we prepend the canonical `/v1` version segment.
    let after_authority = match base.find("://") {
        Some(i) => &base[i + 3..],
        None => base,
    };
    if after_authority.contains('/') {
        format!("{base}{suffix}")
    } else {
        format!("{base}/v1{suffix}")
    }
}

/// Normalizes a SenseNova base URL: bare origins get `/api/v1` prepended
/// (instead of the canonical `/v1` used by standard OpenAI-compatible
/// endpoints). Origins already carrying a path prefix are left untouched.
pub(crate) fn sensenova_base_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return String::new();
    }
    let after_authority = match base.find("://") {
        Some(i) => &base[i + 3..],
        None => base,
    };
    if after_authority.contains('/') {
        base.to_string()
    } else {
        format!("{base}/api/v1")
    }
}

/// Checks an HTTP response for a non-2xx status and, on failure, reads the
/// response body into a `ProviderError::HttpStatus` so the caller sees the
/// provider's error payload (invalid model, expired key, etc.) instead of a
/// bare status code. The body is capped at 4 KiB to keep error messages
/// manageable.
pub(crate) async fn ensure_status(
    response: reqwest::Response,
    url: impl Into<String>,
) -> Result<reqwest::Response, ProviderError> {
    if response.status().is_success() {
        Ok(response)
    } else {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let body = if body.len() > 4096 {
            format!(
                "{}…(truncated)",
                body.chars().take(4096).collect::<String>()
            )
        } else {
            body
        };
        Err(ProviderError::HttpStatus {
            status,
            url: url.into(),
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{join_api_path, sensenova_base_url};

    #[test]
    fn bare_origin_gets_v1_prefix() {
        assert_eq!(
            join_api_path("https://token.sensenova.cn", "/chat/completions"),
            "https://token.sensenova.cn/v1/chat/completions"
        );
    }

    #[test]
    fn v1_suffixed_origin_is_not_doubled() {
        assert_eq!(
            join_api_path("https://api.openai.com/v1", "/chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn trailing_slash_is_trimmed() {
        assert_eq!(
            join_api_path("https://x.example/", "/models"),
            "https://x.example/v1/models"
        );
    }

    #[test]
    fn custom_path_prefix_is_preserved() {
        assert_eq!(
            join_api_path("https://x.example/api/v1", "/models"),
            "https://x.example/api/v1/models"
        );
    }

    #[test]
    fn anthropic_bare_origin_reaches_v1_messages() {
        assert_eq!(
            join_api_path("https://api.anthropic.com", "/messages"),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn anthropic_v1_suffixed_is_not_doubled() {
        assert_eq!(
            join_api_path("https://api.anthropic.com/v1", "/messages"),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn empty_base_yields_bare_suffix() {
        assert_eq!(
            join_api_path("   ", "/chat/completions"),
            "/chat/completions"
        );
    }

    #[test]
    fn sensenova_bare_origin_gets_api_v1_prefix() {
        assert_eq!(
            sensenova_base_url("https://token.sensenova.cn"),
            "https://token.sensenova.cn/api/v1"
        );
    }

    #[test]
    fn sensenova_api_v1_suffixed_is_not_doubled() {
        assert_eq!(
            sensenova_base_url("https://token.sensenova.cn/api/v1"),
            "https://token.sensenova.cn/api/v1"
        );
    }

    #[test]
    fn sensenova_trailing_slash_is_trimmed() {
        assert_eq!(
            sensenova_base_url("https://token.sensenova.cn/"),
            "https://token.sensenova.cn/api/v1"
        );
    }

    #[test]
    fn sensenova_custom_path_is_preserved() {
        assert_eq!(
            sensenova_base_url("https://gw.example.com/sensenova/v1"),
            "https://gw.example.com/sensenova/v1"
        );
    }
}
