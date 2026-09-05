//! Wire-format integration tests for provider clients (mock HTTP only).

use nuomi_core::providers::{
    AnthropicCompatibleClient, ChatMessage, ChatRequest, LlmProvider, OpenAiCompatibleClient,
    ProviderError,
};
use serde_json::json;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn openai_request() -> ChatRequest {
    ChatRequest {
        model: "test-model".into(),
        system_prompt: Some("be brief".into()),
        messages: vec![ChatMessage::user("hello")],
        tools: vec![],
        temperature: Some(0.2),
        max_tokens: Some(128),
        cache_retention: Default::default(),
        cache_scope: None,
    }
}

#[tokio::test]
async fn openai_compatible_complete_roundtrip() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(
            json!({ "model": "test-model", "stream": false }),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": { "role": "assistant", "content": "hi there" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 3, "completion_tokens": 2 }
        })))
        .mount(&server)
        .await;

    let client = OpenAiCompatibleClient::new(server.uri(), "sk-test");
    let resp = client.complete(&openai_request()).await.unwrap();
    assert_eq!(resp.content, "hi there");
    assert_eq!(resp.finish_reason.as_deref(), Some("stop"));
    assert_eq!(resp.usage.map(|u| u.prompt_tokens), Some(3));
}

#[tokio::test]
async fn openai_compatible_stream_emits_text_and_completed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(json!({ "stream": true })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(concat!(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"he\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{\"content\":\"llo\"}}]}\n\n",
                    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: [DONE]\n\n",
                )),
        )
        .mount(&server)
        .await;

    use futures::StreamExt;
    let client = OpenAiCompatibleClient::new(server.uri(), "sk-test");
    let mut stream = client.stream(&openai_request());
    let mut text = String::new();
    let mut completed = None;
    while let Some(ev) = stream.next().await {
        match ev.unwrap() {
            nuomi_core::providers::StreamEvent::TextDelta(d) => text.push_str(&d),
            nuomi_core::providers::StreamEvent::Completed(r) => completed = Some(r),
        }
    }
    assert_eq!(text, "hello");
    assert_eq!(completed.unwrap().content, "hello");
}

#[tokio::test]
async fn openai_compatible_tool_call_parsing() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1", "type": "function",
                        "function": { "name": "delegate_slow", "arguments": "{\"prompt\":\"do x\"}" }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        })))
        .mount(&server)
        .await;

    let client = OpenAiCompatibleClient::new(server.uri(), "k");
    let resp = client.complete(&openai_request()).await.unwrap();
    assert_eq!(resp.tool_calls.len(), 1);
    assert_eq!(resp.tool_calls[0].name, "delegate_slow");
    assert_eq!(resp.tool_calls[0].arguments["prompt"], "do x");
}

#[tokio::test]
async fn anthropic_compatible_complete_and_error_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [
                { "type": "text", "text": "part1 " },
                { "type": "tool_use", "id": "tu_1", "name": "search", "input": { "q": "x" } }
            ],
            "usage": { "input_tokens": 5, "output_tokens": 6 },
            "stop_reason": "tool_use"
        })))
        .mount(&server)
        .await;

    let client = AnthropicCompatibleClient::new(server.uri(), "k");
    let resp = client.complete(&openai_request()).await.unwrap();
    assert_eq!(resp.content, "part1 ");
    assert_eq!(resp.tool_calls[0].arguments["q"], "x");
    assert_eq!(resp.usage.map(|u| u.completion_tokens), Some(6));

    // Error path surfaces as a protocol violation.
    let failing = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "error": { "type": "invalid_request_error", "message": "bad model" }
        })))
        .mount(&failing)
        .await;
    let bad = AnthropicCompatibleClient::new(failing.uri(), "k");
    match bad.complete(&openai_request()).await {
        Err(ProviderError::Protocol { .. }) => {}
        other => panic!("expected protocol error, got {other:?}"),
    }
}
