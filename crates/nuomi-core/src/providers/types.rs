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

/// An image attached to a user message (multimodal input, task 4.1 /
/// spec 5.3.1-2). Translated by each client into its vendor wire format
/// (OpenAI `image_url`, Anthropic `image`); providers without vision
/// support never receive one — the upper layer (steward context builder)
/// degrades to a textual placeholder instead.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatImagePart {
    /// MIME type, e.g. `image/png`.
    pub mime_type: String,
    /// Raw base64 payload (no data-URI prefix).
    pub base64: String,
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
    /// Quoted/referenced event seq for user messages (message quote/reply).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quoted_event_seq: Option<i64>,
    /// Excerpt of the quoted message (auto-extracted by backend).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quoted_excerpt: Option<String>,
    /// Images attached to a user message (multimodal input). Empty for all
    /// existing call sites — `skip_serializing_if` keeps persisted payloads
    /// byte-identical with the pre-multimodal format.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_parts: Vec<ChatImagePart>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            quoted_event_seq: None,
            quoted_excerpt: None,
            image_parts: Vec::new(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            quoted_event_seq: None,
            quoted_excerpt: None,
            image_parts: Vec::new(),
        }
    }
    /// User message with attached images (multimodal input, task 4.1).
    pub fn user_with_images(content: impl Into<String>, image_parts: Vec<ChatImagePart>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            quoted_event_seq: None,
            quoted_excerpt: None,
            image_parts,
        }
    }
    pub fn user_with_quote(
        content: impl Into<String>,
        quoted_event_seq: i64,
        quoted_excerpt: impl Into<String>,
    ) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            quoted_event_seq: Some(quoted_event_seq),
            quoted_excerpt: Some(quoted_excerpt.into()),
            image_parts: Vec::new(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            quoted_event_seq: None,
            quoted_excerpt: None,
            image_parts: Vec::new(),
        }
    }
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Tool,
            content: content.into(),
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id.into()),
            quoted_event_seq: None,
            quoted_excerpt: None,
            image_parts: Vec::new(),
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
    /// CLI Agent 自身会话 id（ADR 0012 D8），由 CLI adapter 提取。
    /// 非 CLI provider 为 None。用于持久化到 session_cli_handles。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_session_id: Option<String>,
}

/// Streaming events normalized across protocols.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    TextDelta(String),
    /// Terminal event with the assembled response.
    Completed(ChatResponse),
}

/// One-shot request. `stream` is implied by which method is called.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub system_prompt: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDef>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i64>,
    /// Prompt-cache retention hint honored by providers that support it.
    pub cache_retention: CacheRetention,
    /// Cache-lineage scope (session lineage root) for providers that route
    /// their prompt cache by an explicit key (OpenAI `prompt_cache_key`).
    /// `None` falls back to provider-default cache routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_scope: Option<String>,
    /// CLI Agent 外部会话 id（ADR 0012 D8），用于让 CLI adapter 在后续轮次
    /// 传入 resume 参数以复用 CLI Agent 自身的模型会话。
    /// 非 CLI provider 忽略此字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_session_id: Option<String>,
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
            cache_scope: None,
            external_session_id: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_message_deserializes_legacy_payload_without_image_parts() {
        // Old persisted JSON (pre-multimodal) carries no image_parts key —
        // `serde(default)` must keep deserialization working.
        let legacy = r#"{"role":"user","content":"hi"}"#;
        let m: ChatMessage = serde_json::from_str(legacy).unwrap();
        assert_eq!(m.role, MessageRole::User);
        assert_eq!(m.content, "hi");
        assert!(m.image_parts.is_empty());
    }

    #[test]
    fn chat_message_skips_image_parts_when_empty() {
        let s = serde_json::to_string(&ChatMessage::user("hi")).unwrap();
        assert_eq!(s, r#"{"role":"user","content":"hi"}"#);
    }

    #[test]
    fn chat_message_roundtrips_image_parts() {
        let m = ChatMessage::user_with_images(
            "看看",
            vec![ChatImagePart {
                mime_type: "image/png".into(),
                base64: "aGk=".into(),
            }],
        );
        let s = serde_json::to_string(&m).unwrap();
        assert!(
            s.contains(r#""image_parts":[{"mime_type":"image/png","base64":"aGk="}]"#),
            "got: {s}"
        );
        let back: ChatMessage = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
    }
}
