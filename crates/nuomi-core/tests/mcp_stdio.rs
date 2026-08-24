//! AC15: stdio MCP server integration tests (local `node -e` ndjson server).

use nuomi_core::harness::{Context, Plugin};
use nuomi_core::plugins::mcp::{McpClient, McpPlugin, McpServerSpec, StdioTransport};
use nuomi_core::plugins::ToolRegistry;
use serde_json::json;
use std::sync::Arc;

/// Minimal ndjson JSON-RPC MCP server: initialize / tools/list / tools/call.
const SERVER_SCRIPT: &str = r#"
const readline = require('readline');
const rl = readline.createInterface({ input: process.stdin, terminal: false });
rl.on('line', (line) => {
  const t = line.trim();
  if (!t) { return; }
  let msg;
  try { msg = JSON.parse(t); } catch { return; }
  let result = null;
  if (msg.method === 'initialize') {
    result = {
      protocolVersion: '2024-11-05',
      capabilities: {},
      serverInfo: { name: 'echo-mcp', version: '0.0.1' },
    };
  } else if (msg.method === 'tools/list') {
    result = {
      tools: [
        { name: 'echo', description: 'echo tool', inputSchema: { type: 'object' } },
      ],
    };
  } else if (msg.method === 'tools/call') {
    result = {
      content: [{ type: 'text', text: 'echo:' + JSON.stringify(msg.params.arguments) }],
    };
  } else {
    return;
  }
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result }) + '\n');
});
"#;

fn spawn_client() -> McpClient {
    // Arg array only — never a shell string.
    let transport =
        StdioTransport::spawn("node", &["-e".to_string(), SERVER_SCRIPT.to_string()], &[])
            .expect("spawn node mcp server");
    McpClient::new("echo-server", Arc::new(transport))
}

#[tokio::test]
async fn stdio_initialize_list_and_call_roundtrip() {
    let client = spawn_client();

    client.initialize().await.expect("initialize");
    let tools = client.list_tools().await.expect("list_tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "echo");
    assert_eq!(tools[0].description, "echo tool");
    assert_eq!(tools[0].parameters, json!({ "type": "object" }));

    let out = client
        .call_tool("echo", &json!({ "x": 1 }))
        .await
        .expect("call_tool");
    assert!(out.contains("echo:"), "unexpected output: {out}");
    assert!(out.contains(r#""x":1"#), "arguments not echoed: {out}");
}

#[tokio::test]
async fn mcp_plugin_registers_stdio_tools_into_registry_and_executes() {
    let ctx = Context::default();
    let registry = Arc::new(ToolRegistry::new());
    ctx.register_service("test", "tools", registry.clone())
        .await
        .expect("register tool registry");

    let plugin = McpPlugin::new(vec![McpServerSpec::Stdio {
        name: "echo-server".into(),
        program: "node".into(),
        args: vec!["-e".into(), SERVER_SCRIPT.to_string()],
        envs: vec![],
    }]);

    plugin
        .init(&ctx)
        .await
        .expect("plugin init discovers tools");

    let out = registry
        .execute("echo", &json!({ "x": 1 }))
        .await
        .expect("execute echo via registry");
    assert!(out.contains("echo:"), "unexpected output: {out}");
    assert!(out.contains(r#""x":1"#), "arguments not echoed: {out}");

    plugin.dispose().await.expect("dispose reaps child");
}
