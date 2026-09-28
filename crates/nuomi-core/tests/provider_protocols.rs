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
        external_session_id: None,
    }
}

#[tokio::test]
async fn openai_compatible_complete_roundtrip() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
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
        .and(path("/v1/chat/completions"))
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

// --- Regression for issue provider-test-conn-404 ---------------------------
// A bare-origin base URL (no `/v1`) must still reach the canonical versioned
// endpoint. Before the fix OpenAiCompatibleClient appended only
// `/chat/completions`, so a bare origin produced a URL missing `/v1` and the
// real provider answered 404.

#[tokio::test]
async fn bare_origin_openai_base_url_reaches_v1_chat_completions() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": { "role": "assistant", "content": "pong" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
        })))
        .mount(&server)
        .await;

    let client = OpenAiCompatibleClient::new(server.uri(), "sk-test");
    let resp = client
        .complete(&openai_request())
        .await
        .expect("bare-origin base_url must reach /v1/chat/completions");
    assert_eq!(resp.content, "pong");
}

#[tokio::test]
async fn v1_suffixed_openai_base_url_does_not_double_v1() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": { "role": "assistant", "content": "ok" },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 1, "completion_tokens": 1 }
        })))
        .mount(&server)
        .await;

    // base_url already carries /v1 — must not become /v1/v1/chat/completions.
    let base = format!("{}/v1", server.uri());
    let client = OpenAiCompatibleClient::new(base, "sk-test");
    let resp = client
        .complete(&openai_request())
        .await
        .expect("/v1-suffixed base_url must not double the /v1 segment");
    assert_eq!(resp.content, "ok");
}

#[tokio::test]
async fn bare_origin_anthropic_base_url_reaches_v1_messages() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{ "type": "text", "text": "hi" }],
            "usage": { "input_tokens": 1, "output_tokens": 1 },
            "stop_reason": "end_turn"
        })))
        .mount(&server)
        .await;

    let client = AnthropicCompatibleClient::new(server.uri(), "k");
    let resp = client
        .complete(&openai_request())
        .await
        .expect("bare-origin anthropic base_url must reach /v1/messages");
    assert_eq!(resp.content, "hi");
}

#[tokio::test]
async fn v1_suffixed_anthropic_base_url_does_not_double_v1() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "content": [{ "type": "text", "text": "hi" }],
            "usage": { "input_tokens": 1, "output_tokens": 1 },
            "stop_reason": "end_turn"
        })))
        .mount(&server)
        .await;

    let base = format!("{}/v1", server.uri());
    let client = AnthropicCompatibleClient::new(base, "k");
    let resp = client
        .complete(&openai_request())
        .await
        .expect("/v1-suffixed anthropic base_url must not double the /v1 segment");
    assert_eq!(resp.content, "hi");
}

// --- Probe-strategy regression for issue provider-test-conn-probe-strategy ---
// Some OpenAI-compatible gateways (e.g. sensenova token endpoints) expose
// `/v1/models` but answer 404 on `/v1/chat/completions` — for tokens scoped
// to listing only, or when chat is routed under a different path. The
// "test connection" button must therefore probe with a models GET (which
// `list_model_ids` does), not a chat-completion POST. This test pins that
// contract: against a server that 404s chat but 200s models, the chat probe
// fails and the models probe succeeds — proving the models GET is the
// correct connectivity strategy.

#[tokio::test]
async fn chat_completion_probe_fails_when_chat_endpoint_is_404() {
    let server = MockServer::start().await;
    // The chat endpoint does not exist on this gateway → 404.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&server)
        .await;

    let client = OpenAiCompatibleClient::new(server.uri(), "sk-test");
    let err = client
        .complete(&openai_request())
        .await
        .expect_err("chat probe must fail when /v1/chat/completions is 404");
    assert!(
        err.to_string().contains("404"),
        "error should mention 404, got: {err}"
    );
}

#[tokio::test]
async fn models_get_probe_succeeds_when_models_endpoint_is_200() {
    use nuomi_core::domain::ProviderProtocol;
    use nuomi_core::providers::list_model_ids;

    let server = MockServer::start().await;
    // The models endpoint is reachable → 200 with a model list.
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "sense-1" }, { "id": "sense-2" }]
        })))
        .mount(&server)
        .await;

    let ids = list_model_ids(
        ProviderProtocol::OpenAiCompatible,
        &server.uri(),
        "sk-test",
        None,
    )
    .await
    .expect("models GET probe must succeed when /v1/models is 200");
    assert_eq!(ids, vec!["sense-1".to_string(), "sense-2".to_string()]);
}

#[tokio::test]
async fn models_get_probe_is_the_correct_strategy_for_404_chat_gateways() {
    // Combined scenario: chat endpoint 404, models endpoint 200 — exactly
    // the sensenova case. The models probe (used by impl_test_provider_connection
    // after the fix) succeeds where a chat probe would have failed.
    use nuomi_core::domain::ProviderProtocol;
    use nuomi_core::providers::list_model_ids;

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [{ "id": "sense-1" }]
        })))
        .mount(&server)
        .await;

    // Chat probe (the old strategy) — fails.
    let chat_err = OpenAiCompatibleClient::new(server.uri(), "sk-test")
        .complete(&openai_request())
        .await
        .expect_err("chat probe must fail");
    assert!(chat_err.to_string().contains("404"), "{chat_err}");

    // Models probe (the new strategy) — succeeds.
    let ids = list_model_ids(
        ProviderProtocol::OpenAiCompatible,
        &server.uri(),
        "sk-test",
        None,
    )
    .await
    .expect("models probe must succeed where chat probe failed");
    assert_eq!(ids, vec!["sense-1".to_string()]);
}
