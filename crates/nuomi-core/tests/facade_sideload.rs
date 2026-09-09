//! Facade ⇄ side-load integration (ADR 0009 §6): `NuomiKernel::boot` must run
//! the real Kernel lifecycle, load side-loaded plugins from configured paths,
//! and expose their tools through the Context-owned ToolRegistry.

use nuomi_core::facade::NuomiKernel;
use nuomi_core::plugins::ToolRegistry;

/// An inline NPP plugin (Node, zero deps) contributing one `upper` tool.
fn write_tool_plugin(dir: &std::path::Path) {
    let script = r#"
const send = (m) => process.stdout.write(JSON.stringify(m) + "\n");
require("readline").createInterface({ input: process.stdin }).on("line", (line) => {
  let msg; try { msg = JSON.parse(line); } catch { return; }
  const { method, id, params } = msg;
  if (method === "initialize") send({ jsonrpc: "2.0", id, result: { api_version: 1, capabilities: { tools: true } } });
  else if (method === "tools/list") send({ jsonrpc: "2.0", id, result: { tools: [{ name: "upper", description: "up", inputSchema: { type: "object", properties: { text: { type: "string" } }, required: ["text"] } }] } });
  else if (method === "tools/call") send({ jsonrpc: "2.0", id, result: { content: [{ type: "text", text: String(params.arguments.text).toUpperCase() }] } });
  else if (method === "shutdown") { send({ jsonrpc: "2.0", id, result: {} }); process.exit(0); }
  else if (id !== undefined) send({ jsonrpc: "2.0", id, error: { code: -32601, message: "no" } });
});
"#;
    let script_path = dir.join("tooler.cjs");
    std::fs::write(&script_path, script).unwrap();
    // TOML literal string (single quotes): backslashes are not escapes.
    std::fs::write(
        dir.join("plugin.toml"),
        format!(
            r#"
id = "tooler"
name = "Tooler"
version = "0.1.0"
api_version = 1
entry = ["node", '{}']
[[tools]]
name = "upper"
description = "up"
"#,
            script_path.display()
        ),
    )
    .unwrap();
}

#[tokio::test]
async fn facade_boots_sideloaded_plugin_and_tool_round_trips() {
    let app_dir = tempfile::tempdir().unwrap();
    let plugin_dir = app_dir.path().join("upper-plugin");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    write_tool_plugin(&plugin_dir);

    let kernel = NuomiKernel::boot(
        nuomi_core::facade::NuomiConfig::with_fake_provider(app_dir.path().join("k.db"), vec![])
            .with_plugin_paths(vec![plugin_dir.clone()]),
    )
    .await
    .unwrap();

    // The boot report records the side-loaded plugin.
    let report = kernel.boot_report();
    assert!(
        report.loaded.iter().any(|id| id == "tooler"),
        "loaded: {:?}",
        report.loaded
    );

    // The plugin's tool is registered (with the mandatory id prefix) and
    // executable through the Context-owned registry.
    let tools = kernel
        .context()
        .service::<ToolRegistry>("tools")
        .await
        .unwrap();
    let defs = tools.defs_for(&[]).await;
    assert!(
        defs.iter().any(|d| d.name == "tooler.upper"),
        "tools: {:?}",
        defs.iter().map(|d| d.name.clone()).collect::<Vec<_>>()
    );
    let output = tools
        .execute(
            "tooler.upper",
            &serde_json::json!({ "text": "hello nuomi" }),
        )
        .await
        .unwrap();
    assert_eq!(output, "HELLO NUOMI");
}

#[tokio::test]
async fn broken_plugin_never_blocks_facade_boot() {
    let app_dir = tempfile::tempdir().unwrap();
    let plugin_dir = app_dir.path().join("broken");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(
        plugin_dir.join("plugin.toml"),
        r#"
id = "broken"
name = "Broken"
version = "0.1.0"
api_version = 1
entry = ["definitely-not-a-real-binary-xyz"]
"#,
    )
    .unwrap();

    let kernel = NuomiKernel::boot(
        nuomi_core::facade::NuomiConfig::with_fake_provider(app_dir.path().join("k.db"), vec![])
            .with_plugin_paths(vec![plugin_dir]),
    )
    .await
    .unwrap();

    // Boot succeeded; the failure is visible in the report with stderr context.
    let report = kernel.boot_report();
    assert!(report.loaded.is_empty(), "{report:?}");
    assert_eq!(report.failed.len(), 1, "{report:?}");
    assert!(
        report.failed[0]
            .1
            .contains("definitely-not-a-real-binary-xyz"),
        "{report:?}"
    );
}
