//! MCP plugin: Model Context Protocol client over stdio and Streamable HTTP.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::harness::{Context, HarnessError, Plugin};
use crate::providers::ToolDef;

use super::tools::{Tool, ToolRegistry};

/// Transport-agnostic JSON-RPC 2.0 channel.
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// Sends a request and waits for the matching response `result`/`error`.
    async fn request(&self, method: &str, params: Value) -> Result<Value, HarnessError>;
    async fn shutdown(&self);
}

fn rpc_error(method: &str, message: impl Into<String>) -> HarnessError {
    HarnessError::PluginFailed {
        plugin: format!("mcp:{method}"),
        phase: "rpc",
        message: message.into(),
    }
}

/// stdio transport: newline-delimited JSON-RPC over a child process.
/// Arguments are passed as an array (never a shell string).
///
/// This is a long-lived transport: one persistent child process with piped
/// stdin/stdout is reused for every request (no per-request spawn), so there
/// is no connection pool to warm — the "connection" is the process itself.
pub struct StdioTransport {
    child: tokio::sync::Mutex<tokio::process::Child>,
    stdin: tokio::sync::Mutex<tokio::process::ChildStdin>,
    stdout: tokio::sync::Mutex<tokio::io::BufReader<tokio::process::ChildStdout>>,
    next_id: AtomicU64,
}

impl StdioTransport {
    pub fn spawn(
        program: &str,
        args: &[String],
        envs: &[(&str, &str)],
    ) -> Result<Self, HarnessError> {
        use std::process::Stdio;
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (k, v) in envs {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().map_err(|e| rpc_error("stdio", e.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| rpc_error("stdio", "no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| rpc_error("stdio", "no stdout"))?;
        Ok(Self {
            child: tokio::sync::Mutex::new(child),
            stdin: tokio::sync::Mutex::new(stdin),
            stdout: tokio::sync::Mutex::new(tokio::io::BufReader::new(stdout)),
            next_id: AtomicU64::new(1),
        })
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, HarnessError> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
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
                return Err(rpc_error(method, "server closed stdout"));
            }
            let Ok(msg) = serde_json::from_str::<Value>(out_line.trim()) else {
                continue;
            };
            if msg.get("id").and_then(Value::as_u64) != Some(id) {
                continue; // notification or stale response
            }
            if let Some(err) = msg.get("error") {
                return Err(rpc_error(method, err.to_string()));
            }
            return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    async fn shutdown(&self) {
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.kill().await;
        }
    }
}

/// Streamable HTTP transport: JSON-RPC over HTTP POST.
///
/// Requests go through the process-wide shared connection pool
/// (`providers::pool::shared_client`), so DNS/TLS/connections are reused
/// with all other HTTP outlets. The previous client-level 60s timeout is
/// preserved as a per-request timeout.
pub struct HttpTransport {
    http: reqwest::Client,
    url: String,
    headers: Vec<(String, String)>,
    next_id: AtomicU64,
}

/// Per-request budget (previously the client-level timeout).
const HTTP_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

impl HttpTransport {
    pub fn new(url: impl Into<String>, headers: Vec<(String, String)>) -> Self {
        Self {
            http: crate::providers::pool::shared_client(),
            url: url.into(),
            headers,
            next_id: AtomicU64::new(1),
        }
    }
}

#[async_trait]
impl McpTransport for HttpTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, HarnessError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let mut req = self
            .http
            .post(&self.url)
            .json(&body)
            .timeout(HTTP_REQUEST_TIMEOUT);
        for (k, v) in &self.headers {
            req = req.header(k.as_str(), v.as_str());
        }
        let resp = req
            .send()
            .await
            .map_err(|e| rpc_error(method, e.to_string()))?;
        let resp = resp
            .error_for_status()
            .map_err(|e| rpc_error(method, e.to_string()))?;
        let msg: Value = resp
            .json()
            .await
            .map_err(|e| rpc_error(method, e.to_string()))?;
        if let Some(err) = msg.get("error") {
            return Err(rpc_error(method, err.to_string()));
        }
        Ok(msg.get("result").cloned().unwrap_or(Value::Null))
    }

    async fn shutdown(&self) {}
}

/// One connected MCP server exposing tools into the kernel registry.
pub struct McpClient {
    pub name: String,
    transport: Arc<dyn McpTransport>,
}

impl McpClient {
    pub fn new(name: impl Into<String>, transport: Arc<dyn McpTransport>) -> Self {
        Self {
            name: name.into(),
            transport,
        }
    }

    pub async fn initialize(&self) -> Result<(), HarnessError> {
        self.transport
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "nuomi", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
            .await
            .map(|_| ())
    }

    /// Discovers remote tools as unified `ToolDef`s.
    pub async fn list_tools(&self) -> Result<Vec<ToolDef>, HarnessError> {
        let result = self.transport.request("tools/list", json!({})).await?;
        let tools = result
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(tools
            .iter()
            .map(|t| ToolDef {
                name: t
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                description: t
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                parameters: t
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or(json!({ "type": "object" })),
            })
            .collect())
    }

    pub async fn call_tool(&self, name: &str, arguments: &Value) -> Result<String, HarnessError> {
        let result = self
            .transport
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await?;
        let content = result
            .get("content")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(content
            .iter()
            .filter_map(|c| c.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// A registry tool backed by a remote MCP server.
struct McpTool {
    def: ToolDef,
    client: Arc<McpClient>,
}

#[async_trait]
impl Tool for McpTool {
    fn def(&self) -> ToolDef {
        self.def.clone()
    }
    async fn execute(&self, arguments: &Value) -> Result<String, HarnessError> {
        self.client.call_tool(&self.def.name, arguments).await
    }
}

/// Server spec consumed by [`McpPlugin`].
pub enum McpServerSpec {
    Stdio {
        name: String,
        program: String,
        args: Vec<String>,
        envs: Vec<(String, String)>,
    },
    Http {
        name: String,
        url: String,
        headers: Vec<(String, String)>,
    },
}

/// Connects configured servers at boot and merges their tools into the
/// shared [`ToolRegistry`].
pub struct McpPlugin {
    specs: Vec<McpServerSpec>,
    clients: tokio::sync::Mutex<Vec<Arc<McpClient>>>,
}

impl McpPlugin {
    pub fn new(specs: Vec<McpServerSpec>) -> Self {
        Self {
            specs,
            clients: tokio::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Plugin for McpPlugin {
    fn id(&self) -> &str {
        "mcp"
    }

    async fn init(&self, ctx: &Context) -> Result<(), HarnessError> {
        let Some(registry) = ctx.service::<ToolRegistry>("tools").await else {
            return Err(HarnessError::PluginNotFound("tool registry".into()));
        };
        let mut clients = self.clients.lock().await;
        for spec in &self.specs {
            let client = match spec {
                McpServerSpec::Stdio {
                    name,
                    program,
                    args,
                    envs,
                } => {
                    let env_refs: Vec<(&str, &str)> =
                        envs.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                    let transport = StdioTransport::spawn(program, args, &env_refs)?;
                    Arc::new(McpClient::new(name.clone(), Arc::new(transport)))
                }
                McpServerSpec::Http { name, url, headers } => Arc::new(McpClient::new(
                    name.clone(),
                    Arc::new(HttpTransport::new(url.clone(), headers.clone())),
                )),
            };
            client.initialize().await?;
            for def in client.list_tools().await? {
                registry
                    .register(Arc::new(McpTool {
                        def,
                        client: client.clone(),
                    }))
                    .await?;
            }
            clients.push(client);
        }
        Ok(())
    }

    async fn dispose(&self) -> Result<(), HarnessError> {
        for c in self.clients.lock().await.iter() {
            c.transport.shutdown().await;
        }
        Ok(())
    }
}
