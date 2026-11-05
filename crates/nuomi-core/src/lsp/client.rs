//! LspClient: JSON-RPC 2.0 over stdio client for Language Server communication.
//!
//! Manages a single LS process: spawns it, sends requests/notifications,
//! receives responses/notifications, and handles the LSP initialize handshake.
//!
//! Architecture:
//! - A background tokio task reads messages from stdout and dispatches them:
//!   - Responses (with `id`) → matched to pending requests via oneshot channels
//!   - Notifications (without `id`) → forwarded to an mpsc channel for consumers
//! - Requests are sent via stdin with a monotonically increasing ID
//! - Each request registers a oneshot::Sender in a pending map; the reader task
//!   resolves it when the matching response arrives

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::{oneshot, Mutex};
use tokio::time::timeout;

use super::transport;
use super::types::{LspLanguageId, LspServerConfig};
use super::LspError;

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>;

/// A JSON-RPC 2.0 message received from the server that is a notification
/// (no `id` field). Consumers receive these via the notification channel.
#[derive(Debug, Clone)]
pub struct ServerNotification {
    pub method: String,
    pub params: Value,
}

/// Handle to a running Language Server process.
pub struct LspClient {
    language: LspLanguageId,
    stdin: Mutex<ChildStdin>,
    child: Mutex<Child>,
    next_id: AtomicU64,
    pending: PendingMap,
    notifications_rx: Mutex<tokio::sync::mpsc::UnboundedReceiver<ServerNotification>>,
    initialized: AtomicBool,
    config: LspServerConfig,
}

impl std::fmt::Debug for LspClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspClient")
            .field("language", &self.language)
            .field("initialized", &self.initialized.load(Ordering::Relaxed))
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl LspClient {
    /// Spawn a Language Server process and start the background reader task.
    pub async fn spawn(config: LspServerConfig) -> Result<Self, LspError> {
        let language = config.language.clone();

        let mut cmd = tokio::process::Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        if let Some(ref dir) = config.working_dir {
            cmd.current_dir(dir);
        }

        for (key, val) in &config.env {
            cmd.env(key, val);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| LspError::ProcessSpawn(format!("{}: {e}", config.command)))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| LspError::ProcessSpawn("no stdin on child process".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LspError::ProcessSpawn("no stdout on child process".into()))?;

        let (notifications_tx, notifications_rx) =
            tokio::sync::mpsc::unbounded_channel::<ServerNotification>();

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let pending_clone = Arc::clone(&pending);

        let reader = BufReader::new(stdout);
        tokio::spawn(async move {
            read_loop(reader, pending_clone, notifications_tx).await;
        });

        Ok(Self {
            language,
            stdin: Mutex::new(stdin),
            child: Mutex::new(child),
            next_id: AtomicU64::new(1),
            pending,
            notifications_rx: Mutex::new(notifications_rx),
            initialized: AtomicBool::new(false),
            config,
        })
    }

    /// Send a JSON-RPC request and wait for the response.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, LspError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let msg = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let (tx, rx) = oneshot::channel::<Value>();
        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }

        let json_str = serde_json::to_string(&msg)
            .map_err(|e| LspError::Protocol(format!("serialize request: {e}")))?;

        {
            let mut stdin = self.stdin.lock().await;
            if let Err(e) = transport::write_message(&mut stdin, &json_str).await {
                let mut pending = self.pending.lock().await;
                pending.remove(&id);
                return Err(e);
            }
        }

        let init_timeout = Duration::from_secs(self.config.init_timeout_secs);
        match timeout(init_timeout, rx).await {
            Ok(Ok(value)) => {
                if let Some(error) = value.get("error") {
                    let code = error.get("code").and_then(|c| c.as_i64()).unwrap_or(-1) as i32;
                    let message = error
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown error")
                        .to_string();
                    Err(LspError::ServerError { code, message })
                } else {
                    Ok(value.get("result").cloned().unwrap_or(Value::Null))
                }
            }
            Ok(Err(_)) => Err(LspError::Protocol(format!(
                "request {method} (id={id}): sender dropped"
            ))),
            Err(_) => {
                let mut pending = self.pending.lock().await;
                pending.remove(&id);
                Err(LspError::Timeout(init_timeout))
            }
        }
    }

    /// Send a JSON-RPC notification (no response expected).
    pub async fn notify(&self, method: &str, params: Value) -> Result<(), LspError> {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });

        let json_str = serde_json::to_string(&msg)
            .map_err(|e| LspError::Protocol(format!("serialize notification: {e}")))?;

        let mut stdin = self.stdin.lock().await;
        transport::write_message(&mut stdin, &json_str).await
    }

    /// Perform the LSP initialize handshake.
    pub async fn initialize(&self, root_uri: &str) -> Result<Value, LspError> {
        let params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "capabilities": {
                "textDocument": {
                    "completion": { "completionItem": { "snippetSupport": true } },
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "signatureHelp": { "signatureInformation": { "parameterInformation": { "labelOffsetSupport": true } } },
                    "synchronization": { "didSave": true, "willSave": false, "willSaveWaitUntil": false },
                    "definition": { "linkSupport": true },
                    "references": {},
                    "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
                    "formatting": {},
                    "diagnostics": {},
                },
                "workspace": {
                    "workspaceFolders": true,
                },
            },
            "workspaceFolders": [{
                "uri": root_uri,
                "name": "workspace",
            }],
        });

        let result = self.request("initialize", params).await?;

        self.notify("initialized", json!({})).await?;
        self.initialized.store(true, Ordering::Relaxed);

        Ok(result)
    }

    /// Send `textDocument/didOpen` notification.
    pub async fn did_open(&self, uri: &str, language_id: &str, text: &str) -> Result<(), LspError> {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": 1,
                    "text": text,
                }
            }),
        )
        .await
    }

    /// Send `textDocument/didChange` notification (full content sync).
    pub async fn did_change(&self, uri: &str, version: i32, text: &str) -> Result<(), LspError> {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": [{ "text": text }],
            }),
        )
        .await
    }

    /// Send `textDocument/didClose` notification.
    pub async fn did_close(&self, uri: &str) -> Result<(), LspError> {
        self.notify(
            "textDocument/didClose",
            json!({
                "textDocument": { "uri": uri }
            }),
        )
        .await
    }

    /// Send `textDocument/didSave` notification.
    pub async fn did_save(&self, uri: &str, text: &str) -> Result<(), LspError> {
        self.notify(
            "textDocument/didSave",
            json!({
                "textDocument": { "uri": uri },
                "text": text,
            }),
        )
        .await
    }

    /// Request `textDocument/completion`.
    pub async fn completion(
        &self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Value, LspError> {
        self.request(
            "textDocument/completion",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            }),
        )
        .await
    }

    /// Request `textDocument/hover`.
    pub async fn hover(&self, uri: &str, line: u32, character: u32) -> Result<Value, LspError> {
        self.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            }),
        )
        .await
    }

    /// Request `textDocument/definition`.
    pub async fn definition(
        &self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Value, LspError> {
        self.request(
            "textDocument/definition",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            }),
        )
        .await
    }

    /// Request `textDocument/references`.
    pub async fn references(
        &self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Value, LspError> {
        self.request(
            "textDocument/references",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "context": { "includeDeclaration": true },
            }),
        )
        .await
    }

    /// Request `textDocument/signatureHelp`.
    pub async fn signature_help(
        &self,
        uri: &str,
        line: u32,
        character: u32,
    ) -> Result<Value, LspError> {
        self.request(
            "textDocument/signatureHelp",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
            }),
        )
        .await
    }

    /// Request `textDocument/formatting`.
    pub async fn formatting(&self, uri: &str) -> Result<Value, LspError> {
        self.request(
            "textDocument/formatting",
            json!({
                "textDocument": { "uri": uri },
                "options": { "tabSize": 4, "insertSpaces": true },
            }),
        )
        .await
    }

    /// Request `textDocument/documentSymbol`.
    pub async fn document_symbols(&self, uri: &str) -> Result<Value, LspError> {
        self.request(
            "textDocument/documentSymbol",
            json!({
                "textDocument": { "uri": uri },
            }),
        )
        .await
    }

    /// Try to receive the next server notification (non-blocking).
    pub async fn try_recv_notification(&self) -> Option<ServerNotification> {
        let mut rx = self.notifications_rx.lock().await;
        rx.try_recv().ok()
    }

    /// Receive the next server notification (blocking, with timeout).
    pub async fn recv_notification(&self, dur: Duration) -> Option<ServerNotification> {
        let mut rx = self.notifications_rx.lock().await;
        timeout(dur, rx.recv()).await.ok().flatten()
    }

    /// Whether the initialize handshake has completed.
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Relaxed)
    }

    /// Language this client serves.
    pub fn language(&self) -> &LspLanguageId {
        &self.language
    }

    /// Shut down the Language Server gracefully.
    pub async fn shutdown(&self) -> Result<(), LspError> {
        if self.is_initialized() {
            let _ = self.request("shutdown", json!({})).await;
            self.notify("exit", json!({})).await.ok();
        }
        let mut child = self.child.lock().await;
        let _ = child.kill().await;
        Ok(())
    }

    /// Check if the child process is still running.
    pub async fn is_alive(&self) -> bool {
        let mut child = self.child.lock().await;
        match child.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) => false,
            Err(_) => false,
        }
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        // Best-effort kill; async shutdown should have been called already.
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.start_kill();
        }
    }
}

/// Background reader loop: reads messages from stdout and dispatches them.
async fn read_loop(
    mut reader: BufReader<ChildStdout>,
    pending: PendingMap,
    notifications_tx: tokio::sync::mpsc::UnboundedSender<ServerNotification>,
) {
    loop {
        match transport::read_message(&mut reader).await {
            Ok(json_str) => {
                let value: Value = match serde_json::from_str(&json_str) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("LSP: invalid JSON from server: {e}");
                        continue;
                    }
                };

                if let Some(id) = value.get("id").and_then(|i| i.as_u64()) {
                    let mut map = pending.lock().await;
                    if let Some(sender) = map.remove(&id) {
                        let _ = sender.send(value);
                    }
                } else if let Some(method) = value.get("method").and_then(|m| m.as_str()) {
                    let params = value.get("params").cloned().unwrap_or(Value::Null);
                    let _ = notifications_tx.send(ServerNotification {
                        method: method.to_string(),
                        params,
                    });
                }
            }
            Err(e) => {
                tracing::info!("LSP reader loop exiting: {e}");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::types::LspServerStatus;

    #[test]
    fn lsp_language_id_display() {
        let id = LspLanguageId::new("python");
        assert_eq!(id.to_string(), "python");
        assert_eq!(id.as_str(), "python");
    }

    #[test]
    fn lsp_server_config_builder() {
        let cfg = LspServerConfig::new("go", "gopls")
            .with_args(vec!["serve".into()])
            .with_enabled(false);

        assert_eq!(cfg.language.as_str(), "go");
        assert_eq!(cfg.command, "gopls");
        assert_eq!(cfg.args, vec!["serve"]);
        assert!(!cfg.enabled);
    }

    #[test]
    fn lsp_server_status_is_ready() {
        assert!(LspServerStatus::Ready.is_ready());
        assert!(!LspServerStatus::NotStarted.is_ready());
        assert!(LspServerStatus::Failed("oops".into()).is_failed());
    }

    #[test]
    fn server_notification_struct() {
        let n = ServerNotification {
            method: "textDocument/publishDiagnostics".into(),
            params: json!({"uri": "file:///test.py"}),
        };
        assert_eq!(n.method, "textDocument/publishDiagnostics");
    }
}
