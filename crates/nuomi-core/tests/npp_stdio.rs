//! End-to-end NPP integration test: boot a real Kernel with built-in plugins
//! plus a side-loaded plugin (the repo's `examples/plugins/upper`, Node
//! variant), then drive a side-loaded tool and a hook through the registries.
//!
//! Modeled on tests/mcp_stdio.rs (requires `node` on PATH, same as the MCP
//! stdio tests).

use std::path::PathBuf;
use std::sync::Arc;

use nuomi_core::harness::sideload::{SideloadedPlugin, Tolerant};
use nuomi_core::harness::{Context, Kernel};
use nuomi_core::plugins::hooks::{HookDecision, HookPoint};
use nuomi_core::plugins::{HooksPlugin, ToolRegistry, ToolsPlugin};

fn upper_plugin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/upper")
}

/// Node entry with an absolute script path so the test does not depend on cwd.
fn node_entry(plugin_dir: &std::path::Path) -> Vec<String> {
    vec![
        "node".into(),
        plugin_dir.join("plugin.cjs").to_string_lossy().into(),
    ]
}

fn manifest_with_node_entry(dir: &std::path::Path) -> nuomi_core::harness::PluginManifest {
    // Reuse the shipped manifest but rewrite entry to absolute node argv.
    let mut manifest = nuomi_core::harness::PluginManifest::load(dir).unwrap();
    manifest.entry = node_entry(dir);
    manifest
}

#[tokio::test]
async fn kernel_boots_with_sideloaded_plugin_and_tool_round_trips() {
    let dir = upper_plugin_dir();
    let manifest = manifest_with_node_entry(&dir);

    let ctx = Context::default();
    let mut kernel = Kernel::new(ctx.clone());
    kernel.register(Arc::new(ToolsPlugin::default())).unwrap();
    kernel
        .register(Arc::new(HooksPlugin::new(Arc::new(
            nuomi_core::plugins::HookRegistry::new(),
        ))))
        .unwrap();
    kernel
        .register(Arc::new(SideloadedPlugin::new(manifest, dir)))
        .unwrap();
    kernel.boot().await.unwrap();

    // The side-loaded tool is registered under "<plugin_id>.<name>".
    let tools = ctx.service::<ToolRegistry>("tools").await.unwrap();
    let defs = tools.defs_for(&[]).await;
    assert!(
        defs.iter().any(|d| d.name == "upper.upper"),
        "tools: {:?}",
        defs.iter().map(|d| d.name.clone()).collect::<Vec<_>>()
    );

    let output = tools
        .execute("upper.upper", &serde_json::json!({ "text": "hello nuomi" }))
        .await
        .unwrap();
    assert_eq!(output, "HELLO NUOMI");

    kernel.shutdown().await;
}

#[tokio::test]
async fn sideloaded_hook_can_deny() {
    // A minimal inline NPP plugin whose hook denies everything, proving the
    // hook/handle round trip carries the verdict back.
    let dir = tempfile::tempdir().unwrap();
    let script = r#"
let n = 0;
const send = (m) => process.stdout.write(JSON.stringify(m) + "\n");
require("readline").createInterface({ input: process.stdin }).on("line", (line) => {
  let msg; try { msg = JSON.parse(line); } catch { return; }
  const { method, id, params } = msg;
  if (method === "initialize") send({ jsonrpc: "2.0", id, result: { api_version: 1, capabilities: { hooks: true } } });
  else if (method === "hook/handle") send({ jsonrpc: "2.0", id, result: { decision: "deny", reason: "blocked-by-test-plugin" } });
  else if (method === "shutdown") { send({ jsonrpc: "2.0", id, result: {} }); process.exit(0); }
  else if (id !== undefined) send({ jsonrpc: "2.0", id, error: { code: -32601, message: "no" } });
});
"#;
    let script_path = dir.path().join("hooker.cjs");
    std::fs::write(&script_path, script).unwrap();
    // TOML literal string (single quotes) so backslashes in the Windows path
    // are not treated as escapes.
    let toml_body = format!(
        r#"
id = "hooker"
name = "Hooker"
version = "0.1.0"
api_version = 1
entry = ["node", '{}']
[[hooks]]
point = "pre_tool_call"
order = 10
"#,
        script_path.display()
    );
    std::fs::write(dir.path().join("plugin.toml"), toml_body).unwrap();

    let ctx = Context::default();
    let mut kernel = Kernel::new(ctx.clone());
    kernel.register(Arc::new(ToolsPlugin::default())).unwrap();
    kernel
        .register(Arc::new(HooksPlugin::new(Arc::new(
            nuomi_core::plugins::HookRegistry::new(),
        ))))
        .unwrap();
    kernel
        .register(Arc::new(SideloadedPlugin::new(
            // The hooker manifest already carries the absolute node entry.
            nuomi_core::harness::PluginManifest::load(dir.path()).unwrap(),
            dir.path().to_path_buf(),
        )))
        .unwrap();
    kernel.boot().await.unwrap();

    let hooks = ctx
        .service::<nuomi_core::plugins::HookRegistry>("hooks")
        .await
        .unwrap();
    let decision = hooks
        .run(HookPoint::PreToolCall, &serde_json::json!({ "tool": "x" }))
        .await;
    assert_eq!(
        decision,
        HookDecision::Deny("blocked-by-test-plugin".into())
    );

    kernel.shutdown().await;
}

#[tokio::test]
async fn failing_sideload_is_non_fatal_via_tolerant() {
    let dir = tempfile::tempdir().unwrap();
    // Valid manifest, but the entry binary does not exist → spawn failure at
    // init. Wrapped in Tolerant the boot must still succeed and the failure
    // must land in the boot report.
    std::fs::write(
        dir.path().join("plugin.toml"),
        r#"
id = "broken"
name = "Broken"
version = "0.1.0"
api_version = 1
entry = ["definitely-not-a-real-binary-xyz"]
"#,
    )
    .unwrap();
    let manifest = nuomi_core::harness::PluginManifest::load(dir.path()).unwrap();

    let report = Arc::new(std::sync::Mutex::new(
        nuomi_core::harness::sideload::BootReport::default(),
    ));
    let mut kernel = Kernel::new(Context::default());
    kernel.register(Arc::new(ToolsPlugin::default())).unwrap();
    kernel
        .register(Arc::new(Tolerant::new(
            Arc::new(SideloadedPlugin::new(manifest, dir.path().to_path_buf())),
            dir.path().display().to_string(),
            Arc::clone(&report),
        )))
        .unwrap();
    // Must not abort boot.
    kernel.boot().await.unwrap();
    let (failed_len, first_failure) = {
        let report = report.lock().unwrap();
        (
            report.failed.len(),
            report.failed.first().map(|(_, r)| r.clone()),
        )
    };
    assert_eq!(failed_len, 1);
    assert!(
        first_failure
            .as_deref()
            .is_some_and(|r| r.contains("definitely-not-a-real-binary")),
        "{first_failure:?}"
    );
    kernel.shutdown().await;
}
