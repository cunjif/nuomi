//! OpenAI-compatible chat completions client (works for OpenAI, DeepSeek,
//! vLLM, Ollama's OpenAI endpoint, etc.).

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};

use super::client::LlmProvider;
use super::sse;
use super::types::{ChatRequest, ChatResponse, MessageRole, StreamEvent, ToolCall, ToolDef, Usage};
use super::{ensure_status, ProviderError};

pub struct OpenAiCompatibleClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl OpenAiCompatibleClient {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>) -> Self {
        Self {
            // Shared process-wide pool: DNS/TLS/connections are reused
            // across every provider client and HTTP outlet.
            http: super::pool::shared_client(),
            base_url: base_url.into(),
            api_key: api_key.into(),
        }
    }

    /// Overrides the HTTP client — used for per-provider proxy routing
    /// (see `pool::client_for_endpoint`).
    pub fn with_http_client(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }

    fn endpoint(&self) -> String {
        // Tolerate both bare origins (`https://host`) and `/v1`-suffixed
        // base URLs — see `super::join_api_path`.
        super::join_api_path(&self.base_url, "/chat/completions")
    }
}

/// Builds the wire body. Exposed for tests.
pub(crate) fn build_body(request: &ChatRequest, stream: bool) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = &request.system_prompt {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for m in &request.messages {
        match m.role {
            MessageRole::System => messages.push(json!({ "role": "system", "content": m.content })),
            MessageRole::User => messages.push(json!({ "role": "user", "content": m.content })),
            MessageRole::Assistant => {
                let mut msg = json!({ "role": "assistant", "content": m.content });
                if !m.tool_calls.is_empty() {
                    msg["tool_calls"] =
                        json!(m.tool_calls.iter().map(|tc| json!({
                        "id": tc.id,
                        "type": "function",
                        "function": { "name": tc.name, "arguments": tc.arguments.to_string() },
                    })).collect::<Vec<_>>());
                }
                messages.push(msg);
            }
            MessageRole::Tool => {
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": m.tool_call_id.clone().unwrap_or_default(),
                    "content": m.content,
                }));
            }
        }
    }

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "stream": stream,
    });
    if !request.tools.is_empty() {
        let tools: Vec<Value> = request.tools.iter().map(|t: &ToolDef| json!({
            "type": "function",
            "function": { "name": t.name, "description": t.description, "parameters": t.parameters },
        })).collect();
        body["tools"] = json!(tools);
    }
    if let Some(t) = request.temperature {
        body["temperature"] = json!(t);
    }
    if let Some(mt) = request.max_tokens {
        body["max_tokens"] = json!(mt);
    }
    // OpenAI routes prompt caches by this key; the session lineage root
    // (hermes cache-lineage) goes here.
    if let Some(scope) = &request.cache_scope {
        body["prompt_cache_key"] = json!(scope);
    }
    body
}

fn parse_tool_call(v: &Value) -> Option<ToolCall> {
    let function = v.get("function")?;
    Some(ToolCall {
        id: v
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        name: function
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        arguments: function
            .get("arguments")
            .and_then(Value::as_str)
            .map(|s| serde_json::from_str(s).unwrap_or(Value::Null))
            .unwrap_or(Value::Null),
    })
}

/// Parses a non-streaming response body.
pub(crate) fn parse_response(body: &Value) -> Result<ChatResponse, ProviderError> {
    let choice = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
        .ok_or(ProviderError::Protocol {
            provider: "openai_compatible",
            message: format!("missing choices in response: {body}"),
        })?;
    let message = choice.get("message").cloned().unwrap_or(Value::Null);
    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(parse_tool_call).collect())
        .unwrap_or_default();
    Ok(ChatResponse {
        content: message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        tool_calls,
        usage: parse_usage(body.get("usage")),
        finish_reason: choice
            .get("finish_reason")
            .and_then(Value::as_str)
            .map(str::to_string),
        cli_session_id: None,
    })
}

fn parse_usage(v: Option<&Value>) -> Option<Usage> {
    let usage = v?;
    Some(Usage {
        prompt_tokens: usage
            .get("prompt_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        completion_tokens: usage
            .get("completion_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        cache_read_tokens: usage
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_i64),
        cache_write_tokens: None,
    })
}

/// Applies one streaming chunk to the accumulating response.
pub(crate) fn apply_chunk(state: &mut ChatResponse, chunk: &Value) {
    let Some(choice) = chunk
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|a| a.first())
    else {
        return;
    };
    if let Some(delta) = choice.get("delta") {
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            state.content.push_str(text);
        }
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                while state.tool_calls.len() <= index {
                    state.tool_calls.push(ToolCall {
                        id: String::new(),
                        name: String::new(),
                        arguments: Value::Null,
                    });
                }
                let slot = &mut state.tool_calls[index];
                if let Some(id) = call.get("id").and_then(Value::as_str) {
                    slot.id.push_str(id);
                }
                if let Some(name) = call
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .and_then(Value::as_str)
                {
                    slot.name.push_str(name);
                }
                if let Some(args) = call
                    .get("function")
                    .and_then(|f| f.get("arguments"))
                    .and_then(Value::as_str)
                {
                    if !matches!(slot.arguments, Value::String(_) | Value::Null) {
                        slot.arguments = Value::Null;
                    }
                    let existing = match std::mem::take(&mut slot.arguments) {
                        Value::String(s) => s,
                        _ => String::new(),
                    };
                    slot.arguments = Value::String(existing + args);
                }
            }
        }
    }
    if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
        state.finish_reason = Some(reason.to_string());
    }
    // Usage may arrive on the final chunk when `stream_options.include_usage`.
    state.usage = parse_usage(chunk.get("usage")).or_else(|| state.usage.take());
}

/// Finalizes accumulated string-typed tool arguments into parsed JSON objects.
pub(crate) fn finalize(state: &mut ChatResponse) {
    for call in &mut state.tool_calls {
        call.arguments = match &call.arguments {
            Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
            other => other.clone(),
        };
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleClient {
    fn id(&self) -> &str {
        "openai_compatible"
    }

    async fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        let response = self
            .http
            .post(self.endpoint())
            .bearer_auth(&self.api_key)
            .json(&build_body(request, false))
            .send()
            .await?;
        let response = ensure_status(response, self.endpoint()).await?;
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
                    .post(url.clone())
                    .bearer_auth(api_key)
                    .json(&body)
                    .send()
                    .await?;
                let response = ensure_status(response, url).await?;
                let mut state = ChatResponse::default();
                let mut emitted = 0usize;
                let mut data = sse::data_payloads(Box::pin(response.bytes_stream()));
                while let Some(payload) = data.next().await {
                    if let Ok(chunk) = serde_json::from_str::<Value>(&payload) {
                        apply_chunk(&mut state, &chunk);
                        if state.content.len() > emitted {
                            let delta = state.content[emitted..].to_string();
                            emitted = state.content.len();
                            let _ = tx.send(Ok(StreamEvent::TextDelta(delta))).await;
                        }
                    }
                }
                finalize(&mut state);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_scope_maps_to_prompt_cache_key_only_when_set() {
        let mut request = ChatRequest::simple("gpt", "sys", "hi");
        request.cache_scope = Some("lineage-root".into());
        let body = build_body(&request, false);
        assert_eq!(body["prompt_cache_key"], "lineage-root");

        let request = ChatRequest::simple("gpt", "sys", "hi");
        let body = build_body(&request, false);
        assert!(body.get("prompt_cache_key").is_none());
    }
}
