//! Third-party plugin side-loading (ADR 0009).
//!
//! A side-loaded plugin is an external process speaking NPP (JSON-RPC 2.0 /
//! ndjson over stdio) declared by a `plugin.toml` manifest. It is wrapped in
//! [`SideloadedPlugin`], which implements the in-process [`Plugin`] trait so
//! built-in and third-party plugins share one lifecycle on the [`Kernel`].
//!
//! Hard rule: side-load failures are never fatal ([`Tolerant`] + `BootReport`).

pub mod loader;
pub mod manifest;
pub mod process;
pub mod protocol;
pub mod report;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::process::{ChildStdin, ChildStdout};
use tokio::sync::Mutex;

use super::{Context, HarnessError, Plugin};
use crate::plugins::hooks::{HookDecision, HookRegistry};
use crate::plugins::tools::{Tool, ToolRegistry};
use crate::providers::ToolDef;

pub use loader::{scan, LoadOutcome, SourceKind};
pub use manifest::{Permissions, PluginManifest};
pub use process::StderrTail;
pub use protocol::NPP_API_VERSION;
pub use report::BootReport;

use manifest::HookPointSpec;
use process::PluginProcess;
use protocol::NppConnection;

/// Concrete connection over a plugin child process's piped stdio.
type ChildNpp = NppConnection<BufReader<ChildStdout>, ChildStdin>;

/// Handshake/init budget: a plugin that does not answer within this window
/// lands in the boot report as failed and boot continues.
const INIT_TIMEOUT: Duration = Duration::from_secs(5);
/// Hook verdict budget; timeout ⇒ allow (hooks must not block the run).
const HOOK_TIMEOUT: Duration = Duration::from_secs(5);
/// Grace for the `shutdown` handshake before the process is killed.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

/// Matches an `[[events]].topic` pattern against a bus topic. Accepts `*`,
/// exact, dot-prefix (`session`) and the documented `session.*` spelling.
fn topic_matches(pattern: &str, topic: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern == "*" {
        return true;
    }
    let prefix = pattern.strip_suffix(".*").unwrap_or(pattern);
    if prefix.is_empty() {
        return true;
    }
    topic == prefix
        || topic
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('.'))
}

/// A tool contributed by a side-loaded plugin: forwards `tools/call` over NPP.
struct NppTool {
    /// Fully-qualified registry name: `<plugin_id>.<tool_name>`.
    qualified_name: String,
    /// Bare name the plugin knows.
    bare_name: String,
    description: String,
    parameters: Value,
    conn: Arc<ChildNpp>,
    timeout: Duration,
}

#[async_trait]
impl Tool for NppTool {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: self.qualified_name.clone(),
            description: self.description.clone(),
            parameters: self.parameters.clone(),
        }
    }

    async fn execute(&self, arguments: &Value) -> Result<String, HarnessError> {
        let params = json!({ "name": self.bare_name, "arguments": arguments });
        let result = tokio::time::timeout(self.timeout, self.conn.request("tools/call", params))
            .await
            .map_err(|_| HarnessError::PluginFailed {
                plugin: self.qualified_name.clone(),
                phase: "tools/call",
                message: format!("timed out after {}ms", self.timeout.as_millis()),
            })??;
        extract_text(&result).ok_or_else(|| HarnessError::PluginFailed {
            plugin: self.qualified_name.clone(),
            phase: "tools/call",
            message: "malformed result: expected content[].text".into(),
        })
    }
}

/// NPP tools/call results are MCP-shaped: `content: [{type:"text", text}]`.
fn extract_text(result: &Value) -> Option<String> {
    let content = result.get("content")?.as_array()?;
    let mut text = String::new();
    for item in content {
        if item.get("type").and_then(Value::as_str) == Some("text") {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(item.get("text")?.as_str()?);
        }
    }
    Some(text)
}

/// Formats the captured stderr of a plugin process for error/report context.
fn format_stderr_tail(process: &PluginProcess) -> String {
    let tail = process.stderr_tail();
    let tail = tail
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if tail.is_empty() {
        return String::new();
    }
    format!(
        " [stderr: {}]",
        tail.iter().cloned().collect::<Vec<_>>().join(" | ")
    )
}

/// One side-loaded plugin: manifest + child process + NPP connection.
pub struct SideloadedPlugin {
    manifest: PluginManifest,
    dir: PathBuf,
    process: Mutex<Option<PluginProcess>>,
    conn: Mutex<Option<Arc<ChildNpp>>>,
}

impl SideloadedPlugin {
    /// Wraps a parsed manifest. Nothing is spawned until `init`.
    pub fn new(manifest: PluginManifest, dir: PathBuf) -> Self {
        Self {
            manifest,
            dir,
            process: Mutex::new(None),
            conn: Mutex::new(None),
        }
    }

    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn err(&self, phase: &'static str, message: impl Into<String>) -> HarnessError {
        HarnessError::PluginFailed {
            plugin: self.manifest.id.clone(),
            phase,
            message: message.into(),
        }
    }

    async fn register_tools(
        &self,
        ctx: &Context,
        conn: &Arc<ChildNpp>,
    ) -> Result<(), HarnessError> {
        if self.manifest.tools.is_empty() {
            return Ok(());
        }
        let listed = tokio::time::timeout(INIT_TIMEOUT, conn.request("tools/list", json!({})))
            .await
            .map_err(|_| self.err("tools/list", "timed out during init"))??;
        let offered: Vec<&Value> = listed
            .get("tools")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .collect();
        let registry = ctx
            .service::<ToolRegistry>("tools")
            .await
            .ok_or_else(|| self.err("tools/list", "tool registry service missing"))?;
        for contribution in &self.manifest.tools {
            // The manifest is the permission boundary: only declared tools
            // are registered, matched against what the plugin offered.
            let offered_tool = offered.iter().find(|t| {
                t.get("name").and_then(Value::as_str) == Some(contribution.name.as_str())
            });
            let Some(offered_tool) = offered_tool else {
                return Err(self.err(
                    "tools/list",
                    format!(
                        "declared tool '{}' not offered by the plugin",
                        contribution.name
                    ),
                ));
            };
            let parameters = offered_tool
                .get("inputSchema")
                .cloned()
                .filter(|v| v.is_object())
                .unwrap_or_else(|| contribution.input.clone());
            let tool = NppTool {
                qualified_name: format!("{}.{}", self.manifest.id, contribution.name),
                bare_name: contribution.name.clone(),
                description: offered_tool
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or(&contribution.description)
                    .to_string(),
                parameters,
                conn: Arc::clone(conn),
                timeout: Duration::from_millis(contribution.timeout_ms),
            };
            registry.register(Arc::new(tool)).await?;
        }
        Ok(())
    }

    async fn register_hooks(
        &self,
        ctx: &Context,
        conn: &Arc<ChildNpp>,
    ) -> Result<(), HarnessError> {
        if self.manifest.hooks.is_empty() {
            return Ok(());
        }
        let registry = ctx
            .service::<HookRegistry>("hooks")
            .await
            .ok_or_else(|| self.err("hooks", "hook registry service missing"))?;
        for contribution in &self.manifest.hooks {
            let Some(point) = HookPointSpec::parse(&contribution.point) else {
                // Validated at manifest load; defensive only.
                return Err(self.err("hooks", format!("unknown point '{}'", contribution.point)));
            };
            let plugin_id = self.manifest.id.clone();
            let hook_point = contribution.point.clone();
            let conn = Arc::clone(conn);
            registry
                .add(point.to_kernel(), contribution.order, move |payload: &Value| {
                    let conn = Arc::clone(&conn);
                    let hook_point = hook_point.clone();
                    let plugin_id = plugin_id.clone();
                    let payload = payload.clone();
                    Box::pin(async move {
                        let params = json!({ "point": hook_point, "payload": payload });
                        match tokio::time::timeout(HOOK_TIMEOUT, conn.request("hook/handle", params))
                            .await
                        {
                            Err(_) => {
                                tracing::warn!(plugin = %plugin_id, "hook/handle timed out; allowing");
                                HookDecision::Allow
                            }
                            Ok(Err(e)) => {
                                tracing::warn!(plugin = %plugin_id, "hook/handle failed: {e}; allowing");
                                HookDecision::Allow
                            }
                            Ok(Ok(result)) => {
                                match result.get("decision").and_then(Value::as_str) {
                                    Some("deny") => HookDecision::Deny(
                                        result
                                            .get("reason")
                                            .and_then(Value::as_str)
                                            .unwrap_or("denied by plugin")
                                            .to_string(),
                                    ),
                                    _ => HookDecision::Allow,
                                }
                            }
                        }
                    })
                })
                .await;
        }
        Ok(())
    }

    /// Spawns forwarder tasks: bus topics matching the manifest patterns are
    /// pushed to the plugin as `event` notifications (fire-and-forget).
    async fn spawn_event_forwarders(&self, ctx: &Context, conn: &Arc<ChildNpp>) {
        if self.manifest.events.is_empty() {
            return;
        }
        let patterns: Vec<String> = self
            .manifest
            .events
            .iter()
            .map(|e| e.topic.clone())
            .collect();
        let plugin_id = self.manifest.id.clone();
        let conn = Arc::clone(conn);
        let mut rx = ctx.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if patterns.iter().any(|p| topic_matches(p, &event.topic)) {
                            let params = json!({ "topic": event.topic, "payload": event.payload });
                            if let Err(e) = conn.notify("event", params).await {
                                tracing::debug!(plugin = %plugin_id, "event forward ended: {e}");
                                return;
                            }
                        }
                    }
                    // Bus closed (shutdown) or lagged: end the forwarder.
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });
    }
}

#[async_trait]
impl Plugin for SideloadedPlugin {
    fn id(&self) -> &str {
        &self.manifest.id
    }

    async fn init(&self, ctx: &Context) -> Result<(), HarnessError> {
        let argv = self.manifest.resolved_entry(&self.dir);
        let (process, stdio) = PluginProcess::spawn(&argv, &self.dir)?;
        let conn = Arc::new(ChildNpp::new(BufReader::new(stdio.stdout), stdio.stdin));
        // Handshake under a strict budget; a hung plugin must not block boot.
        let host_version = env!("CARGO_PKG_VERSION");
        match tokio::time::timeout(INIT_TIMEOUT, conn.initialize(host_version)).await {
            Err(_) => {
                let detail = format_stderr_tail(&process);
                process.kill().await;
                return Err(self.err("initialize", format!("timed out during init{detail}")));
            }
            Ok(Err(e)) => {
                let detail = format_stderr_tail(&process);
                process.kill().await;
                return Err(self.err("initialize", format!("{e}{detail}")));
            }
            Ok(Ok(_)) => {}
        }
        self.register_tools(ctx, &conn).await?;
        self.register_hooks(ctx, &conn).await?;
        self.spawn_event_forwarders(ctx, &conn).await;
        *self.process.lock().await = Some(process);
        *self.conn.lock().await = Some(conn);
        Ok(())
    }

    async fn start(&self) -> Result<(), HarnessError> {
        let conn = self.conn.lock().await;
        if let Some(conn) = conn.as_ref() {
            conn.notify("initialized", json!({}))
                .await
                .map_err(|e| self.err("start", e.to_string()))?;
        }
        Ok(())
    }

    async fn dispose(&self) -> Result<(), HarnessError> {
        let conn = self.conn.lock().await.take();
        let process = self.process.lock().await.take();
        if let Some(conn) = conn {
            match tokio::time::timeout(SHUTDOWN_GRACE, conn.request("shutdown", json!({}))).await {
                Ok(Ok(_)) | Ok(Err(_)) | Err(_) => {} // Shutdown is best-effort by protocol; kill below is the backstop.
            }
        }
        if let Some(process) = process {
            process.kill().await;
        }
        Ok(())
    }
}

/// Kernel adapter making one side-loaded plugin's failures non-fatal: errors
/// from `init`/`start` are recorded into the [`BootReport`] and swallowed so
/// the rest of the boot (built-ins first, other plugins next) proceeds.
pub struct Tolerant {
    inner: Arc<dyn Plugin>,
    /// Human-readable label for reports (the plugin directory).
    label: String,
    report: Arc<std::sync::Mutex<BootReport>>,
}

impl Tolerant {
    pub fn new(
        inner: Arc<dyn Plugin>,
        label: impl Into<String>,
        report: Arc<std::sync::Mutex<BootReport>>,
    ) -> Self {
        Self {
            inner,
            label: label.into(),
            report,
        }
    }
}

#[async_trait]
impl Plugin for Tolerant {
    fn id(&self) -> &str {
        self.inner.id()
    }

    async fn init(&self, ctx: &Context) -> Result<(), HarnessError> {
        match self.inner.init(ctx).await {
            Ok(()) => {
                self.report
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .record_loaded(self.inner.id());
                Ok(())
            }
            Err(e) => {
                self.report
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .record_failed(self.label.clone(), e.to_string());
                Ok(())
            }
        }
    }

    async fn start(&self) -> Result<(), HarnessError> {
        match self.inner.start().await {
            Ok(()) => Ok(()),
            Err(e) => {
                self.report
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .record_failed(self.label.clone(), e.to_string());
                Ok(())
            }
        }
    }

    async fn dispose(&self) -> Result<(), HarnessError> {
        self.inner.dispose().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_patterns_match_bus_semantics() {
        assert!(topic_matches("*", "anything"));
        assert!(topic_matches("", "anything"));
        assert!(topic_matches("session.*", "session.message"));
        assert!(topic_matches("session.*", "session.delta.batch"));
        assert!(topic_matches("session", "session.message"));
        assert!(topic_matches("session.message", "session.message"));
        assert!(!topic_matches("session.*", "task.status_changed"));
        assert!(!topic_matches("session", "sessions.list"));
        assert!(!topic_matches("session.message", "session"));
    }

    #[test]
    fn extract_text_concatenates_text_content() {
        let result = json!({"content": [
            {"type": "text", "text": "a"},
            {"type": "image", "data": "…"},
            {"type": "text", "text": "b"},
        ]});
        assert_eq!(extract_text(&result).unwrap(), "a\nb");
        assert!(extract_text(&json!({})).is_none());
    }
}
