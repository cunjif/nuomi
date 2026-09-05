//! Anthropic-compatible messages client.

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};

use super::client::LlmProvider;
use super::sse;
use super::types::{
    CacheRetention, ChatRequest, ChatResponse, MessageRole, StreamEvent, ToolCall, Usage,
};
use super::ProviderError;

pub struct AnthropicCompatibleClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl AnthropicCompatibleClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key: api_key.into(),
        }
    }

    fn endpoint(&self) -> String {
        format!("{}/v1/messages", self.base_url.trim_end_matches('/'))
    }
}

fn cache_control(retention: CacheRetention) -> Value {
    match retention {
        CacheRetention::Long => json!({ "type": "ephemeral", "ttl": "1h" }),
        _ => json!({ "type": "ephemeral" }),
    }
}

/// Marks the system prompt and the last message with `cache_control` so the
/// provider caches the stable prefix (system + latest turn boundary).
fn apply_cache_marks(body: &mut Value, retention: CacheRetention) {
    let marker = cache_control(retention);
    if let Some(system) = body
        .get("system")
        .and_then(Value::as_str)
        .map(str::to_string)
    {
        body["system"] = json!([{
            "type": "text",
            "text": system,
            "cache_control": marker,
        }]);
    }
    if let Some(last) = body
        .get_mut("messages")
        .and_then(Value::as_array_mut)
        .and_then(|a| a.last_mut())
    {
        match last.get_mut("content") {
            Some(Value::Array(blocks)) => {
                if let Some(block) = blocks.last_mut() {
                    block["cache_control"] = marker;
                }
            }
            Some(Value::String(text)) => {
                let text = text.clone();
                last["content"] =
                    json!([{ "type": "text", "text": text, "cache_control": marker }]);
            }
            _ => {}
        }
    }
}

/// Builds the wire body. Exposed for tests.
pub(crate) fn build_body(request: &ChatRequest, stream: bool) -> Value {
    let mut messages = Vec::new();
    for m in &request.messages {
        match m.role {
            MessageRole::System | MessageRole::User => {
                messages.push(json!({ "role": "user", "content": m.content }));
            }
            MessageRole::Tool => {
                // Tool results are user-turn content blocks in Anthropic protocol.
                messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                        "content": m.content,
                    }],
                }));
            }
            MessageRole::Assistant => {
                let mut content = Vec::new();
                if !m.content.is_empty() {
                    content.push(json!({ "type": "text", "text": m.content }));
                }
                for tc in &m.tool_calls {
                    content.push(json!({
                        "type": "tool_use", "id": tc.id, "name": tc.name, "input": tc.arguments,
                    }));
                }
                if content.is_empty() {
                    content.push(json!({ "type": "text", "text": "" }));
                }
                messages.push(json!({ "role": "assistant", "content": content }));
            }
        }
    }

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": stream,
    });
    if let Some(system) = &request.system_prompt {
        body["system"] = json!(system);
    }
    if !request.tools.is_empty() {
        body["tools"] = json!(request
            .tools
            .iter()
            .map(|t| json!({
                "name": t.name, "description": t.description, "input_schema": t.parameters,
            }))
            .collect::<Vec<_>>());
    }
    if let Some(t) = request.temperature {
        body["temperature"] = json!(t);
    }
    if let Some(mt) = request.max_tokens {
        body["max_tokens"] = json!(mt);
    } else {
        // Anthropic requires max_tokens.
        body["max_tokens"] = json!(4096);
    }
    if request.cache_retention != CacheRetention::None {
        apply_cache_marks(&mut body, request.cache_retention);
    }
    body
}

fn parse_usage(v: Option<&Value>) -> Option<Usage> {
    let usage = v?;
    Some(Usage {
        prompt_tokens: usage
            .get("input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        completion_tokens: usage
            .get("output_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        cache_read_tokens: usage.get("cache_read_input_tokens").and_then(Value::as_i64),
        cache_write_tokens: usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_i64),
    })
}

/// Parses a non-streaming response body.
pub(crate) fn parse_response(body: &Value) -> Result<ChatResponse, ProviderError> {
    if let Some(err) = body.get("error") {
        return Err(ProviderError::Protocol {
            provider: "anthropic_compatible",
            message: err.to_string(),
        });
    }
    let content = body
        .get("content")
        .and_then(Value::as_array)
        .ok_or(ProviderError::Protocol {
            provider: "anthropic_compatible",
            message: format!("missing content array: {body}"),
        })?;
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    for block in content {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    text.push_str(t);
                }
            }
            Some("tool_use") => {
                tool_calls.push(ToolCall {
                    id: block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    arguments: block.get("input").cloned().unwrap_or(Value::Null),
                });
            }
            _ => {}
        }
    }
    Ok(ChatResponse {
        content: text,
        tool_calls,
        usage: parse_usage(body.get("usage")),
        finish_reason: body
            .get("stop_reason")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

#[async_trait]
impl LlmProvider for AnthropicCompatibleClient {
    fn id(&self) -> &str {
        "anthropic_compatible"
    }

    async fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        let response = self
            .http
            .post(self.endpoint())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&build_body(request, false))
            .send()
            .await?
            .error_for_status()?;
        let body: Value = response.json().await?;
        parse_response(&body)
    }

    fn stream(
        &self,
        request: &ChatRequest,
    ) -> BoxStream<'static, Result<StreamEvent, ProviderError>> {
        let http = self.http.clone();
        let url = self.endpoint();
        let api_key = self.api_key.clone();
        let body = build_body(request, true);

        let (mut tx, rx) =
            futures::channel::mpsc::channel::<Result<StreamEvent, ProviderError>>(64);
        tokio::spawn(async move {
            async fn run(
                http: reqwest::Client,
                url: String,
                api_key: String,
                body: Value,
                tx: &mut futures::channel::mpsc::Sender<Result<StreamEvent, ProviderError>>,
            ) -> Result<(), ProviderError> {
                let response = http
                    .post(url)
                    .header("x-api-key", api_key)
                    .header("anthropic-version", "2023-06-01")
                    .json(&body)
                    .send()
                    .await?
                    .error_for_status()?;
                let mut state = ChatResponse::default();
                let mut emitted = 0usize;
                let mut data = sse::data_payloads(Box::pin(response.bytes_stream()));
                while let Some(payload) = data.next().await {
                    let Ok(event) = serde_json::from_str::<Value>(&payload) else {
                        continue;
                    };
                    match event.get("type").and_then(Value::as_str) {
                        Some("content_block_delta") => {
                            let delta = event.get("delta");
                            if let Some(text) =
                                delta.and_then(|d| d.get("text")).and_then(Value::as_str)
                            {
                                state.content.push_str(text);
                                let new_len = state.content.len();
                                let piece = state.content[emitted..].to_string();
                                emitted = new_len;
                                let _ = tx.send(Ok(StreamEvent::TextDelta(piece))).await;
                            }
                            if let Some(partial) = delta
                                .and_then(|d| d.get("partial_json"))
                                .and_then(Value::as_str)
                            {
                                // Tool-use input arrives as JSON fragments; buffer
                                // into a synthetic pending call slot.
                                if state.tool_calls.is_empty() {
                                    state.tool_calls.push(ToolCall {
                                        id: String::new(),
                                        name: String::new(),
                                        arguments: Value::Null,
                                    });
                                }
                                let Some(slot) = state.tool_calls.last_mut() else {
                                    continue;
                                };
                                if !matches!(slot.arguments, Value::String(_) | Value::Null) {
                                    slot.arguments = Value::Null;
                                }
                                let existing = match std::mem::take(&mut slot.arguments) {
                                    Value::String(s) => s,
                                    _ => String::new(),
                                };
                                slot.arguments = Value::String(existing + partial);
                            }
                        }
                        Some("content_block_start") => {
                            let block = event.get("content_block");
                            if block.and_then(|b| b.get("type")).and_then(Value::as_str)
                                == Some("tool_use")
                            {
                                state.tool_calls.push(ToolCall {
                                    id: block
                                        .and_then(|b| b.get("id"))
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string(),
                                    name: block
                                        .and_then(|b| b.get("name"))
                                        .and_then(Value::as_str)
                                        .unwrap_or_default()
                                        .to_string(),
                                    arguments: Value::Null,
                                });
                            }
                        }
                        Some("message_delta") => {
                            if let Some(reason) = event
                                .get("delta")
                                .and_then(|d| d.get("stop_reason"))
                                .and_then(Value::as_str)
                            {
                                state.finish_reason = Some(reason.to_string());
                            }
                            state.usage =
                                parse_usage(event.get("usage")).or_else(|| state.usage.take());
                        }
                        _ => {}
                    }
                }
                finalize_tool_args(&mut state);
                let _ = tx.send(Ok(StreamEvent::Completed(state))).await;
                Ok(())
            }

            if let Err(e) = run(http, url, api_key, body, &mut tx).await {
                let _ = tx.send(Err(e)).await;
            }
        });

        rx.boxed()
    }
}

fn finalize_tool_args(state: &mut ChatResponse) {
    for call in &mut state.tool_calls {
        call.arguments = match &call.arguments {
            Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
            other => other.clone(),
        };
    }
}
