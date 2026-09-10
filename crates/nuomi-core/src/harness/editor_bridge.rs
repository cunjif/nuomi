//! Editor bridge: runtime RPC channel from the Tauri shell to side-loaded
//! plugins that declare an `[editor]` section (see `sideload/manifest.rs`).
//!
//! The registry is a Context-owned service (qualifier `editor_bridge`, the
//! shared ADR 0009 §6 convention). `SideloadedPlugin::init` registers its NPP
//! connection here when the manifest declares editor contributions; the
//! shell's `plugin_editor_call` IPC resolves the service and forwards a call.
//!
//! Security boundary: only methods explicitly declared by the plugin manifest
//! are forwarded (`editor/hover`, `editor/symbols`) or mapped (`editor/command`
//! → the manifest-declared tool via `tools/call`). Nothing else reaches the
//! plugin process.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncWrite};
use tokio::sync::RwLock;

use super::sideload::manifest::EditorSection;
use super::sideload::protocol::NppConnection;
use super::HarnessError;

/// Transport-agnostic NPP request surface. Object-safe so the registry can
/// hold live child-process connections and tests can hold in-memory duplex
/// connections behind one type.
#[async_trait]
pub trait EditorRpc: Send + Sync {
    async fn rpc_request(&self, method: &str, params: Value) -> Result<Value, HarnessError>;
}

#[async_trait]
impl<R, W> EditorRpc for NppConnection<R, W>
where
    R: AsyncBufRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    async fn rpc_request(&self, method: &str, params: Value) -> Result<Value, HarnessError> {
        self.request(method, params).await
    }
}

/// Budget for one editor RPC round-trip (hover/symbols/command). Mirrors the
/// hook budget: editor providers must never wedge the UI for long.
pub const EDITOR_RPC_TIMEOUT: Duration = Duration::from_secs(5);

struct EditorBridgeEntry {
    conn: Arc<dyn EditorRpc>,
    section: EditorSection,
}

/// Maps plugin ids to their live NPP connection + declared editor section.
#[derive(Default)]
pub struct EditorBridgeRegistry {
    inner: RwLock<HashMap<String, EditorBridgeEntry>>,
}

impl EditorBridgeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a live plugin connection with its declared contributions.
    /// A later registration for the same plugin id wins (kernel ids are
    /// unique per boot, so this is defensive only).
    pub async fn register(
        &self,
        plugin_id: String,
        conn: Arc<dyn EditorRpc>,
        section: EditorSection,
    ) {
        self.inner
            .write()
            .await
            .insert(plugin_id, EditorBridgeEntry { conn, section });
    }

    /// Removes a plugin's entry (called from `SideloadedPlugin::dispose`).
    pub async fn remove(&self, plugin_id: &str) {
        self.inner.write().await.remove(plugin_id);
    }

    /// True when the plugin is live on the bridge (introspection/tests).
    pub async fn is_registered(&self, plugin_id: &str) -> bool {
        self.inner.read().await.contains_key(plugin_id)
    }

    /// Forwards one editor RPC. `editor/command` is transparently mapped to
    /// the manifest-declared `tools/call` so the plugin sees only tools it
    /// already published.
    pub async fn call(
        &self,
        plugin_id: &str,
        method: &str,
        params: Value,
    ) -> Result<Value, HarnessError> {
        let entry = {
            let map = self.inner.read().await;
            let entry = map
                .get(plugin_id)
                .ok_or_else(|| HarnessError::PluginNotFound(plugin_id.to_string()))?;
            EditorBridgeEntry {
                conn: Arc::clone(&entry.conn),
                section: entry.section.clone(),
            }
        };
        let err = |message: String| HarnessError::PluginFailed {
            plugin: plugin_id.to_string(),
            phase: "editor",
            message,
        };
        // Every accepted method resolves to one (wire method, params) pair and
        // then falls through to a single timeout-guarded call — `editor/command`
        // is rewritten here into the manifest-declared `tools/call`, so the
        // plugin never sees an NPP method it did not publish.
        let (wire_method, wire_params): (&str, Value) = match method {
            "editor/hover" if entry.section.hover => (method, params),
            "editor/symbols" if entry.section.symbols => (method, params),
            "editor/command" if !entry.section.commands.is_empty() => {
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| err("editor/command params missing 'name'".into()))?;
                let command = entry
                    .section
                    .commands
                    .iter()
                    .find(|c| c.name == name)
                    .ok_or_else(|| {
                        err(format!("command '{name}' is not declared by the manifest"))
                    })?;
                (
                    "tools/call",
                    json!({
                        "name": command.tool,
                        "arguments": params.get("arguments").cloned().unwrap_or(json!({})),
                    }),
                )
            }
            _ => {
                return Err(err(format!(
                    "method '{method}' is not declared by the manifest [editor] section"
                )));
            }
        };
        tokio::time::timeout(
            EDITOR_RPC_TIMEOUT,
            entry.conn.rpc_request(wire_method, wire_params),
        )
        .await
        .map_err(|_| {
            err(format!(
                "{method} timed out after {}ms",
                EDITOR_RPC_TIMEOUT.as_millis()
            ))
        })?
        .map_err(|e| err(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{duplex, BufReader, DuplexStream};

    type TestConn =
        crate::harness::sideload::protocol::NppConnection<BufReader<DuplexStream>, DuplexStream>;

    /// Drives a scripted plugin side of the duplex: replies to the first
    /// request with `reply` (echoing its id).
    async fn scripted_plugin(
        mut plugin_read: DuplexStream,
        mut plugin_write: DuplexStream,
        reply: Value,
    ) {
        use tokio::io::AsyncReadExt;
        let mut buf = vec![0u8; 8192];
        let n = plugin_read.read(&mut buf).await.unwrap();
        let req: Value = serde_json::from_slice(&buf[..n]).unwrap();
        let res = json!({"jsonrpc":"2.0","id":req["id"],"result":reply});
        use tokio::io::AsyncWriteExt;
        plugin_write
            .write_all(format!("{res}\n").as_bytes())
            .await
            .unwrap();
    }

    fn demo_section() -> EditorSection {
        EditorSection {
            languages: vec!["*".into()],
            hover: true,
            symbols: false,
            commands: vec![
                super::super::sideload::manifest::EditorCommandContribution {
                    name: "ask".into(),
                    title: "Ask".into(),
                    tool: "to-upper".into(),
                },
            ],
            overlays: vec![],
        }
    }

    fn conn_pair() -> (Arc<TestConn>, DuplexStream, DuplexStream) {
        let (host_read, plugin_write) = duplex(4096);
        let (plugin_read, host_write) = duplex(4096);
        (
            Arc::new(TestConn::new(BufReader::new(host_read), host_write)),
            plugin_read,
            plugin_write,
        )
    }

    #[tokio::test]
    async fn hover_call_forwards_to_declared_plugin() {
        let registry = EditorBridgeRegistry::new();
        let (conn, plugin_read, plugin_write) = conn_pair();
        registry
            .register("upper".into(), conn, demo_section())
            .await;
        assert!(registry.is_registered("upper").await);
        let driver = tokio::spawn(scripted_plugin(
            plugin_read,
            plugin_write,
            json!({"contents": "**UPPER**"}),
        ));
        let result = registry
            .call("upper", "editor/hover", json!({"line": 1}))
            .await
            .unwrap();
        driver.await.unwrap();
        assert_eq!(result["contents"], "**UPPER**");
    }

    #[tokio::test]
    async fn undeclared_method_is_rejected() {
        let registry = EditorBridgeRegistry::new();
        let (conn, _plugin_read, _plugin_write) = conn_pair();
        let mut s = demo_section();
        s.hover = false;
        registry.register("upper".into(), conn, s).await;
        let err = registry
            .call("upper", "editor/hover", json!({}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not declared"), "{err}");
    }

    #[tokio::test]
    async fn unknown_plugin_is_not_found() {
        let registry = EditorBridgeRegistry::new();
        let err = registry
            .call("ghost", "editor/hover", json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, HarnessError::PluginNotFound(_)), "{err}");
    }

    #[tokio::test]
    async fn command_call_maps_to_declared_tool() {
        let registry = EditorBridgeRegistry::new();
        let (conn, mut plugin_read, mut plugin_write) = conn_pair();
        registry
            .register("upper".into(), conn, demo_section())
            .await;
        let driver = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = vec![0u8; 8192];
            let n = plugin_read.read(&mut buf).await.unwrap();
            let req: Value = serde_json::from_slice(&buf[..n]).unwrap();
            // The bridge must translate editor/command → tools/call with the
            // manifest-declared bare tool name.
            assert_eq!(req["method"], "tools/call");
            assert_eq!(req["params"]["name"], "to-upper");
            assert_eq!(req["params"]["arguments"]["text"], "hi");
            let res = json!({"jsonrpc":"2.0","id":req["id"],"result":{"content":[{"type":"text","text":"HI"}]}});
            plugin_write
                .write_all(format!("{res}\n").as_bytes())
                .await
                .unwrap();
        });
        let result = registry
            .call(
                "upper",
                "editor/command",
                json!({"name": "ask", "arguments": {"text": "hi"}}),
            )
            .await
            .unwrap();
        driver.await.unwrap();
        assert_eq!(result["content"][0]["text"], "HI");
    }

    #[tokio::test]
    async fn undeclared_command_is_rejected() {
        let registry = EditorBridgeRegistry::new();
        let (conn, _plugin_read, _plugin_write) = conn_pair();
        registry
            .register("upper".into(), conn, demo_section())
            .await;
        let err = registry
            .call("upper", "editor/command", json!({"name": "ghost"}))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not declared"), "{err}");
    }

    #[tokio::test]
    async fn remove_drops_the_entry() {
        let registry = EditorBridgeRegistry::new();
        let (conn, _plugin_read, _plugin_write) = conn_pair();
        registry
            .register("upper".into(), conn, demo_section())
            .await;
        registry.remove("upper").await;
        assert!(!registry.is_registered("upper").await);
        assert!(registry
            .call("upper", "editor/hover", json!({}))
            .await
            .is_err());
    }
}
