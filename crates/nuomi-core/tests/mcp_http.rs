//! AC15: Streamable-HTTP MCP transport integration tests (wiremock, no real network).

use nuomi_core::harness::{Context, Plugin};
use nuomi_core::plugins::mcp::{HttpTransport, McpClient, McpPlugin, McpServerSpec};
use nuomi_core::plugins::ToolRegistry;
use serde_json::{json, Value};
use std::sync::Arc;
use wiremock::matchers::{body_partial_json, method};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn rpc_result(result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": 1, "result": result })
}

async fn mount_echo_server() -> MockServer {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "initialize" })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(rpc_result(json!({ "protocolVersion": "2024-11-05" }))),
        )
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "tools/list" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(rpc_result(json!({
            "tools": [
                { "name": "echo", "description": "echo tool", "inputSchema": { "type": "object" } }
            ]
        }))))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(body_partial_json(json!({ "method": "tools/call" })))
        .respond_with(|req: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
            let args = body["params"]["arguments"].clone();
            ResponseTemplate::new(200).set_body_json(rpc_result(json!({
                "content": [{ "type": "text", "text": format!("echo:{args}") }]
            })))
        })
        .mount(&server)
        .await;

    server
}

#[tokio::test]
async fn http_initialize_list_and_call_roundtrip() {
    let server = mount_echo_server().await;

    let client = McpClient::new(
        "http-echo",
        Arc::new(HttpTransport::new(format!("{}/mcp", server.uri()), vec![])),
    );

    client.initialize().await.expect("initialize");
    let tools = client.list_tools().await.expect("list_tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");
    assert_eq!(tools[0].description, "echo tool");

    let out = client
        .call_tool("echo", &json!({ "x": 1 }))
        .await
        .expect("call_tool");
    assert!(out.contains("echo:"), "unexpected output: {out}");
    assert!(out.contains(r#""x":1"#), "arguments not echoed: {out}");
}

#[tokio::test]
async fn mcp_plugin_registers_http_tools_into_registry_and_executes() {
    let server = mount_echo_server().await;

    let ctx = Context::default();
    let registry = Arc::new(ToolRegistry::new());
    ctx.register_service("test", "tools", registry.clone())
        .await
        .expect("register tool registry");

    let plugin = McpPlugin::new(vec![McpServerSpec::Http {
        name: "http-echo".into(),
        url: format!("{}/mcp", server.uri()),
        headers: vec![],
    }]);

    plugin
        .init(&ctx)
        .await
        .expect("plugin init discovers tools");

    let out = registry
        .execute("echo", &json!({ "msg": "hi" }))
        .await
        .expect("execute echo via registry");
    assert!(out.contains("echo:"), "unexpected output: {out}");
    assert!(out.contains("hi"), "arguments not echoed: {out}");

    plugin.dispose().await.expect("dispose");
}
