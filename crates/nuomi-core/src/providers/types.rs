//! Unified chat protocol types shared by all provider clients.
//!
//! These are the kernel-facing types; each client translates to/from its
//! vendor wire format.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

/// A tool call emitted by the model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// JSON object of arguments.
    pub arguments: serde_json::Value,
}

/// A message in the unified conversation format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    /// Assistant messages may carry parallel tool calls.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// For `Tool` role: the id of the call this message answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id.into()),
        }
    }
}

/// A callable tool advertised to the model (JSON-Schema parameters).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    /// JSON schema object.
    pub parameters: serde_json::Value,
}

/// Token usage reported by the provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    /// Prompt tokens served from the provider's prompt cache, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<i64>,
    /// Prompt tokens written to the provider's prompt cache, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<i64>,
}

/// How long the provider should retain the prompt cache for a request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CacheRetention {
    /// Do not request prompt caching.
    #[default]
    None,
    /// Short-lived cache (provider default TTL, e.g. Anthropic 5m).
    Short,
    /// Long-lived cache (e.g. Anthropic 1h).
    Long,
}

/// A fully-buffered model response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<Usage>,
    /// Provider's finish reason when available (`stop`, `tool_use`, ...).
    pub finish_reason: Option<String>,
}

/// Streaming events normalized across protocols.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    TextDelta(String),
    /// Terminal event with the assembled response.
    Completed(ChatResponse),
}

/// One-shot request. `stream` is implied by which method is called.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub model: String,
    pub system_prompt: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDef>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i64>,
    /// Prompt-cache retention hint honored by providers that support it.
    pub cache_retention: CacheRetention,
}

impl ChatRequest {
    pub fn simple(model: &str, system: &str, user: &str) -> Self {
        Self {
            model: model.to_string(),
            system_prompt: Some(system.to_string()),
            messages: vec![ChatMessage::user(user)],
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            cache_retention: CacheRetention::None,
        }
    }
}
