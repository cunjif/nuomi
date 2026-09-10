//! NPP wire layer: JSON-RPC 2.0 over ndjson stdio (ADR 0009).
//!
//! Framing matches the MCP `StdioTransport` precedent (one JSON message per
//! line, `BufReader::read_line`), with one addition: inbound *notifications*
//! from the plugin (`nuomi/log`) are handled inline in the read loop instead
//! of being skipped, so plugin logs surface while we wait for a response.

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use super::super::HarnessError;

/// The protocol major this host speaks. Plugins offering a higher
/// `api_version` are rejected at the `initialize` handshake.
///
/// Method surface (v1, additive — see ADR 0009 + ADR 0010):
/// - `initialize` / `initialized` (notify) / `shutdown`: lifecycle handshake
/// - `tools/list` / `tools/call`: tool contributions
/// - `hook/handle`: hook verdicts
/// - `event` (notify): bus topics the manifest subscribed to
/// - `nuomi/log` (plugin→host): log forwarding
/// - `editor/hover` / `editor/symbols`: editor providers, only sent to
///   plugins whose manifest `[editor]` section declares `hover` / `symbols`
/// - `editor/command`: never sent as such — the host maps it to the
///   manifest-declared `tools/call` (see `editor_bridge.rs`)
pub const NPP_API_VERSION: u32 = 1;

/// The host's own identity, sent in `initialize`.
pub const HOST_NAME: &str = "nuomi";

fn rpc_error(method: &str, message: impl Into<String>) -> HarnessError {
    HarnessError::PluginFailed {
        plugin: format!("npp:{method}"),
        phase: "rpc",
        message: message.into(),
    }
}

/// Bidirectional NPP connection. Generic over the IO halves so tests can drive
/// it over in-memory duplexes; child-process use goes through [`ProcessIo`].
pub struct NppConnection<R, W>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    stdin: tokio::sync::Mutex<W>,
    stdout: tokio::sync::Mutex<R>,
    next_id: AtomicU64,
}

impl<R, W> NppConnection<R, W>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    pub fn new(stdout: R, stdin: W) -> Self {
        Self {
            stdin: tokio::sync::Mutex::new(stdin),
            stdout: tokio::sync::Mutex::new(stdout),
            next_id: AtomicU64::new(1),
        }
    }

    /// Sends a request and awaits the matching response. Inbound plugin
    /// notifications seen while waiting are handled, never skipped silently.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, HarnessError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let line = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        {
            let mut stdin = self.stdin.lock().await;
            stdin
                .write_all(format!("{line}\n").as_bytes())
                .await
                .map_err(|e| rpc_error(method, e.to_string()))?;
            stdin
                .flush()
                .await
                .map_err(|e| rpc_error(method, e.to_string()))?;
        }
        loop {
            let mut out_line = String::new();
            let n = {
                let mut stdout = self.stdout.lock().await;
                stdout
                    .read_line(&mut out_line)
                    .await
                    .map_err(|e| rpc_error(method, e.to_string()))?
            };
            if n == 0 {
                return Err(rpc_error(method, "plugin closed stdout"));
            }
            let Ok(msg) = serde_json::from_str::<Value>(out_line.trim()) else {
                tracing::warn!("npp: dropping unparseable plugin output: {out_line}");
                continue;
            };
            if msg.get("id").and_then(Value::as_u64) != Some(id) {
                // Inbound notification or a response to something else.
                if msg.get("method").is_some() && msg.get("id").is_none() {
                    Self::handle_inbound_notification(&msg);
                }
                continue;
            }
            if let Some(err) = msg.get("error") {
                return Err(rpc_error(method, err.to_string()));
            }
            return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    /// Fire-and-forget notification (no response expected).
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), HarnessError> {
        let line = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(|e| rpc_error(method, e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| rpc_error(method, e.to_string()))
    }

    /// The `initialize` handshake: sends host identity, validates that the
    /// plugin does not speak a newer protocol major than we do.
    pub async fn initialize(&self, host_version: &str) -> Result<Value, HarnessError> {
        let result = self
            .request(
                "initialize",
                json!({
                    "api_version": NPP_API_VERSION,
                    "host": { "name": HOST_NAME, "version": host_version },
                    "capabilities": {},
                }),
            )
            .await?;
        let offered = result.get("api_version").and_then(Value::as_u64);
        match offered {
            Some(v) if v <= u64::from(NPP_API_VERSION) => Ok(result),
            Some(v) => Err(rpc_error(
                "initialize",
                format!("plugin speaks api_version {v}, host speaks <= {NPP_API_VERSION}"),
            )),
            None => Err(rpc_error(
                "initialize",
                "plugin response missing numeric api_version",
            )),
        }
    }

    fn handle_inbound_notification(msg: &Value) {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        if method == "nuomi/log" {
            let level = params
                .get("level")
                .and_then(Value::as_str)
                .unwrap_or("info");
            let message = params.get("message").and_then(Value::as_str).unwrap_or("");
            match level {
                "error" => tracing::error!(target: "nuomi::plugin", "{message}"),
                "warn" => tracing::warn!(target: "nuomi::plugin", "{message}"),
                _ => tracing::info!(target: "nuomi::plugin", "{message}"),
            }
        } else {
            tracing::debug!(target: "nuomi::plugin", "unhandled plugin notification '{method}'");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{BufReader, DuplexStream};

    type TestConn = NppConnection<BufReader<DuplexStream>, DuplexStream>;

    fn duplex_pair() -> (TestConn, DuplexStream, DuplexStream) {
        // host_read/plugin_write and plugin_read/host_write. The host side
        // reads through a BufReader (NppConnection requires AsyncBufRead).
        let (host_read, plugin_write) = tokio::io::duplex(4096);
        let (plugin_read, host_write) = tokio::io::duplex(4096);
        (
            NppConnection::new(BufReader::new(host_read), host_write),
            plugin_read,
            plugin_write,
        )
    }

    #[tokio::test]
    async fn request_round_trip_and_initialize_validation() {
        let (conn, mut plugin_read, mut plugin_write) = duplex_pair();
        let driver = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = vec![0u8; 4096];
            // initialize
            let n = plugin_read.read(&mut buf).await.unwrap();
            let req: Value = serde_json::from_slice(&buf[..n]).unwrap();
            assert_eq!(req["method"], "initialize");
            assert_eq!(req["params"]["api_version"], 1);
            let res = json!({"jsonrpc":"2.0","id":1,"result":{"api_version":1,"capabilities":{}}});
            plugin_write
                .write_all(format!("{res}\n").as_bytes())
                .await
                .unwrap();
            // second request with an inbound notification interleaved
            let n = plugin_read.read(&mut buf).await.unwrap();
            let req: Value = serde_json::from_slice(&buf[..n]).unwrap();
            assert_eq!(req["method"], "tools/list");
            let log = json!({"jsonrpc":"2.0","method":"nuomi/log","params":{"level":"info","message":"hi"}});
            plugin_write
                .write_all(format!("{log}\n").as_bytes())
                .await
                .unwrap();
            let res = json!({"jsonrpc":"2.0","id":2,"result":{"tools":[]}});
            plugin_write
                .write_all(format!("{res}\n").as_bytes())
                .await
                .unwrap();
        });
        let init = conn.initialize("0.1.0").await.unwrap();
        assert_eq!(init["api_version"], 1);
        let tools = conn.request("tools/list", json!({})).await.unwrap();
        assert_eq!(tools["tools"], json!([]));
        driver.await.unwrap();
    }

    #[tokio::test]
    async fn newer_plugin_api_version_is_rejected() {
        let (conn, mut plugin_read, mut plugin_write) = duplex_pair();
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = vec![0u8; 4096];
            let _ = plugin_read.read(&mut buf).await.unwrap();
            let res = json!({"jsonrpc":"2.0","id":1,"result":{"api_version":2,"capabilities":{}}});
            plugin_write
                .write_all(format!("{res}\n").as_bytes())
                .await
                .unwrap();
        });
        let err = conn.initialize("0.1.0").await.unwrap_err();
        assert!(err.to_string().contains("api_version 2"), "{err}");
    }

    #[tokio::test]
    async fn plugin_error_response_maps_to_harness_error() {
        let (conn, mut plugin_read, mut plugin_write) = duplex_pair();
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = vec![0u8; 4096];
            let _ = plugin_read.read(&mut buf).await.unwrap();
            let res = json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"nope"}});
            plugin_write
                .write_all(format!("{res}\n").as_bytes())
                .await
                .unwrap();
        });
        let err = conn.request("tools/list", json!({})).await.unwrap_err();
        assert!(err.to_string().contains("nope"), "{err}");
    }

    #[test]
    fn notification_without_id_is_handled_inline() {
        // Direct unit test of the router (the async paths cover it too).
        let msg =
            json!({"jsonrpc":"2.0","method":"nuomi/log","params":{"level":"warn","message":"m"}});
        TestConn::handle_inbound_notification(&msg);
        let weird = json!({"jsonrpc":"2.0","method":"ghost/method","params":{}});
        TestConn::handle_inbound_notification(&weird);
    }
}
