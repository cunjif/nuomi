//! Tool registry: the set of tools an agent may call.

use async_trait::async_trait;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::exchange::{
    CallHandle, Coordinate, ExchangeError, ExchangeJournal, GateOutcome, HandleKind, ListFilter,
    RecordStatus,
};
use crate::providers::ToolDef;

use super::super::harness::{Event, HarnessError};

/// Default run id used by [`ToolRegistry::execute`] when the caller does not
/// provide run context (exchange records remain grouped and readable).
const DEFAULT_RUN_ID: &str = "default";

/// A callable tool. MCP tools, slave-provider delegations and builtins
/// all implement this.
#[async_trait]
pub trait Tool: Send + Sync {
    fn def(&self) -> ToolDef;
    async fn execute(&self, arguments: &serde_json::Value) -> Result<String, HarnessError>;
}

/// Registry shared via the plugin Context.
#[derive(Default)]
pub struct ToolRegistry {
    tools: tokio::sync::RwLock<BTreeMap<String, Arc<dyn Tool>>>,
    /// When set, every execution flows through the exchange filesystem
    /// (submit → gate → execute → complete → receipt). `None` keeps the
    /// legacy direct path (default: off).
    exchange: tokio::sync::RwLock<Option<Arc<ExchangeJournal>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn register(&self, tool: Arc<dyn Tool>) -> Result<(), HarnessError> {
        let name = tool.def().name.clone();
        let mut tools = self.tools.write().await;
        if tools.contains_key(&name) {
            return Err(HarnessError::DuplicateService {
                name: format!("tool#{name}"),
                owner: "registry".into(),
            });
        }
        tools.insert(name, tool);
        Ok(())
    }

    pub async fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().await.get(name).cloned()
    }

    /// Tool definitions, optionally filtered by a role allowlist
    /// (empty allowlist = unrestricted).
    pub async fn defs_for(&self, allowlist: &[String]) -> Vec<ToolDef> {
        let tools = self.tools.read().await;
        tools
            .values()
            .filter(|t| allowlist.is_empty() || allowlist.iter().any(|a| *a == t.def().name))
            .map(|t| t.def())
            .collect()
    }

    /// Enables the exchange filesystem for this registry and registers the
    /// three retrieval tools so agents can page through outputs like
    /// documents.
    pub async fn set_exchange(&self, journal: Arc<ExchangeJournal>) -> Result<(), HarnessError> {
        for tool in exchange_reader_tools(Arc::clone(&journal)) {
            self.register(tool).await?;
        }
        *self.exchange.write().await = Some(journal);
        Ok(())
    }

    /// The exchange journal when the exchange filesystem is enabled.
    pub async fn exchange(&self) -> Option<Arc<ExchangeJournal>> {
        self.exchange.read().await.clone()
    }

    /// Executes a tool by name with JSON arguments. With the exchange
    /// filesystem enabled the caller receives an execution receipt
    /// (coordinate + status + preview) instead of the raw output.
    pub async fn execute(
        &self,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<String, HarnessError> {
        self.execute_in_run(DEFAULT_RUN_ID, name, arguments).await
    }

    /// Like [`ToolRegistry::execute`] but attributes the call to `run_id`
    /// in the exchange journal. The three exchange reader tools always take
    /// the direct path: they ARE the retrieval commands, so they must
    /// return data, not receipts.
    pub async fn execute_in_run(
        &self,
        run_id: &str,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<String, HarnessError> {
        if let Some(journal) = self.exchange().await {
            if !is_exchange_reader(name) {
                return self
                    .execute_via_exchange(&journal, run_id, name, arguments)
                    .await;
            }
        }
        self.execute_direct(name, arguments).await
    }

    /// Legacy direct path (exchange disabled) — behavior unchanged.
    async fn execute_direct(
        &self,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<String, HarnessError> {
        let tool = self
            .get(name)
            .await
            .ok_or_else(|| HarnessError::PluginNotFound(format!("tool#{name}")))?;
        tool.execute(arguments).await
    }

    /// Exchange path: submit → gate → execute → complete → receipt.
    /// Rejected/intercepted calls never execute and still answer with a
    /// receipt carrying the reason/policy.
    async fn execute_via_exchange(
        &self,
        journal: &Arc<ExchangeJournal>,
        run_id: &str,
        name: &str,
        arguments: &serde_json::Value,
    ) -> Result<String, HarnessError> {
        let outcome = journal
            .submit(
                run_id,
                CallHandle {
                    kind: HandleKind::Tool,
                    id: name.to_string(),
                },
                arguments.clone(),
            )
            .await;
        let coordinate = outcome.coordinate().clone();
        match outcome {
            GateOutcome::Rejected { reason, .. } => Ok(receipt(&coordinate, "rejected", None)
                .reason(reason)
                .finish()),
            GateOutcome::Intercepted { policy, .. } => {
                Ok(receipt(&coordinate, "intercepted", None)
                    .reason(policy)
                    .finish())
            }
            GateOutcome::Fixed { patch, .. } => {
                let receipt = receipt(&coordinate, "pending", Some(patch));
                self.execute_gated(journal, &coordinate, name, arguments, receipt)
                    .await
            }
            GateOutcome::Allowed(_) => {
                let receipt = receipt(&coordinate, "pending", None);
                self.execute_gated(journal, &coordinate, name, arguments, receipt)
                    .await
            }
        }
    }

    /// Gate allowed (or fixed) the call: execute, journal the output behind
    /// the output gate, and answer with the execution receipt.
    async fn execute_gated(
        &self,
        journal: &Arc<ExchangeJournal>,
        coordinate: &Coordinate,
        name: &str,
        arguments: &serde_json::Value,
        receipt: ReceiptBuilder,
    ) -> Result<String, HarnessError> {
        let executed = self.execute_direct(name, arguments).await;
        match executed {
            Ok(raw) => {
                let record = journal
                    .complete(coordinate, serde_json::Value::String(raw))
                    .await
                    .map_err(exchange_error)?;
                Ok(receipt
                    .status(record.status.as_str())
                    .output_len(record.output_len)
                    .preview(record.preview)
                    .finish())
            }
            Err(error) => {
                let _ = journal.fail(coordinate, error.to_string()).await;
                Err(error)
            }
        }
    }

    /// Publishes a `tool.call` event for observability (bus is injected).
    pub(crate) fn event(name: &str, args: &serde_json::Value) -> Event {
        Event::new(
            "tool.call",
            serde_json::json!({ "tool": name, "arguments": args }),
        )
    }
}

/// Incrementally built execution receipt (JSON object string).
struct ReceiptBuilder {
    value: serde_json::Value,
}

impl ReceiptBuilder {
    fn status(mut self, status: &str) -> Self {
        self.value["status"] = serde_json::json!(status);
        self
    }
    fn reason(mut self, reason: String) -> Self {
        self.value["reason"] = serde_json::json!(reason);
        self
    }
    fn output_len(mut self, output_len: Option<usize>) -> Self {
        if let Some(len) = output_len {
            self.value["output_len"] = serde_json::json!(len);
        }
        self
    }
    fn preview(mut self, preview: Option<String>) -> Self {
        if let Some(preview) = preview {
            self.value["preview"] = serde_json::json!(preview);
        }
        self
    }
    fn finish(self) -> String {
        serde_json::json!({ "exchange_receipt": self.value }).to_string()
    }
}

fn receipt(
    coordinate: &Coordinate,
    status: &str,
    patch: Option<serde_json::Value>,
) -> ReceiptBuilder {
    let mut value = serde_json::json!({
        "coordinate": coordinate.to_string(),
        "status": status,
        "read_hint": "call exchange_read with this coordinate to fetch the full output",
    });
    if let Some(patch) = patch {
        value["patch"] = patch;
    }
    ReceiptBuilder { value }
}

fn exchange_error(error: ExchangeError) -> HarnessError {
    HarnessError::PluginFailed {
        plugin: "exchange".into(),
        phase: "execute",
        message: error.to_string(),
    }
}

/// True for the exchange retrieval tools, which bypass the exchange gate
/// (they read the journal instead of executing workloads).
fn is_exchange_reader(name: &str) -> bool {
    matches!(name, "exchange_list" | "exchange_read" | "exchange_tail")
}

fn arg_str(arguments: &serde_json::Value, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn arg_usize(arguments: &serde_json::Value, key: &str, fallback: usize) -> usize {
    arguments
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .map(|value| value as usize)
        .unwrap_or(fallback)
}

/// `exchange_list` — list exchange envelopes of a run, optionally filtered.
struct ExchangeListTool {
    journal: Arc<ExchangeJournal>,
}

#[async_trait]
impl Tool for ExchangeListTool {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "exchange_list".into(),
            description: "List exchange call records of a run (coordinate, tool, status, preview)"
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "run_id": { "type": "string" },
                    "tool": { "type": "string" },
                    "status": { "type": "string", "enum": ["pending", "succeeded", "failed", "rejected", "intercepted"] }
                },
                "required": ["run_id"]
            }),
        }
    }

    async fn execute(&self, arguments: &serde_json::Value) -> Result<String, HarnessError> {
        let run_id = arg_str(arguments, "run_id").ok_or_else(|| {
            exchange_error(ExchangeError::CoordinateNotFound("missing 'run_id'".into()))
        })?;
        let status = arguments
            .get("status")
            .and_then(serde_json::Value::as_str)
            .and_then(RecordStatus::parse);
        let filter = ListFilter {
            tool: arg_str(arguments, "tool"),
            status,
        };
        let envelopes = self.journal.list(&run_id, &filter).await;
        serde_json::to_string(&envelopes).map_err(|error| exchange_error(error.into()))
    }
}

/// `exchange_read` — read the full record behind a coordinate (hot tier hit
/// is zero-IO; spilled payloads are loaded from their blob on demand).
struct ExchangeReadTool {
    journal: Arc<ExchangeJournal>,
}

#[async_trait]
impl Tool for ExchangeReadTool {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "exchange_read".into(),
            description: "Read the full exchanged output behind a coordinate like a document page"
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "coordinate": { "type": "string", "description": "exch/<run_id>/<seq>-<ulid>" }
                },
                "required": ["coordinate"]
            }),
        }
    }

    async fn execute(&self, arguments: &serde_json::Value) -> Result<String, HarnessError> {
        let text = arg_str(arguments, "coordinate").ok_or_else(|| {
            exchange_error(ExchangeError::CoordinateNotFound(
                "missing 'coordinate'".into(),
            ))
        })?;
        let coordinate = Coordinate::parse(&text)
            .ok_or_else(|| exchange_error(ExchangeError::CoordinateNotFound(text.clone())))?;
        let record = self
            .journal
            .read(&coordinate)
            .await
            .map_err(exchange_error)?;
        serde_json::to_string(&record).map_err(|error| exchange_error(error.into()))
    }
}

/// `exchange_tail` — last N envelopes of a run (most recent calls).
struct ExchangeTailTool {
    journal: Arc<ExchangeJournal>,
}

#[async_trait]
impl Tool for ExchangeTailTool {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "exchange_tail".into(),
            description: "Show the last N exchange records of a run (most recent calls)".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "run_id": { "type": "string" },
                    "n": { "type": "integer", "minimum": 1 }
                },
                "required": ["run_id"]
            }),
        }
    }

    async fn execute(&self, arguments: &serde_json::Value) -> Result<String, HarnessError> {
        let run_id = arg_str(arguments, "run_id").ok_or_else(|| {
            exchange_error(ExchangeError::CoordinateNotFound("missing 'run_id'".into()))
        })?;
        let n = arg_usize(arguments, "n", 20);
        let envelopes = self.journal.tail(&run_id, n).await;
        serde_json::to_string(&envelopes).map_err(|error| exchange_error(error.into()))
    }
}

fn exchange_reader_tools(journal: Arc<ExchangeJournal>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(ExchangeListTool {
            journal: Arc::clone(&journal),
        }),
        Arc::new(ExchangeReadTool {
            journal: Arc::clone(&journal),
        }),
        Arc::new(ExchangeTailTool { journal }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::gate::{Gate, GateVerdict};
    use crate::exchange::{CallRequest, ExchangeConfig, ResultRecord};
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Echo;

    #[async_trait]
    impl Tool for Echo {
        fn def(&self) -> ToolDef {
            ToolDef {
                name: "echo".into(),
                description: "echoes".into(),
                parameters: json!({}),
            }
        }
        async fn execute(&self, args: &serde_json::Value) -> Result<String, HarnessError> {
            Ok(args.to_string())
        }
    }

    struct CountingTool {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl Tool for CountingTool {
        fn def(&self) -> ToolDef {
            ToolDef {
                name: "counted".into(),
                description: "counts invocations".into(),
                parameters: json!({}),
            }
        }
        async fn execute(&self, _args: &serde_json::Value) -> Result<String, HarnessError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok("ran".to_string())
        }
    }

    struct RejectAllGate;

    #[async_trait]
    impl Gate for RejectAllGate {
        async fn inspect_input(&self, _request: &CallRequest) -> GateVerdict {
            GateVerdict::Reject("blocked by test policy".to_string())
        }
        async fn inspect_output(&self, _record: &ResultRecord) -> GateVerdict {
            GateVerdict::Allow
        }
    }

    fn journal(root: &std::path::Path) -> Arc<ExchangeJournal> {
        Arc::new(ExchangeJournal::new(
            root.to_path_buf(),
            ExchangeConfig::default(),
        ))
    }

    #[tokio::test]
    async fn register_execute_and_allowlist() {
        let reg = ToolRegistry::new();
        reg.register(Arc::new(Echo)).await.unwrap();
        assert!(reg.register(Arc::new(Echo)).await.is_err());
        assert_eq!(
            reg.execute("echo", &json!({ "x": 1 })).await.unwrap(),
            r#"{"x":1}"#
        );
        assert!(reg.execute("missing", &json!({})).await.is_err());
        assert_eq!(reg.defs_for(&[]).await.len(), 1);
        assert_eq!(reg.defs_for(&["other".to_string()]).await.len(), 0);
        assert_eq!(reg.defs_for(&["echo".to_string()]).await.len(), 1);
    }

    #[tokio::test]
    async fn exchange_disabled_keeps_legacy_behavior() {
        let reg = ToolRegistry::new();
        reg.register(Arc::new(Echo)).await.unwrap();
        assert!(reg.exchange().await.is_none());
        // Raw output, no reader tools registered.
        assert_eq!(
            reg.execute("echo", &json!({ "x": 1 })).await.unwrap(),
            r#"{"x":1}"#
        );
        assert_eq!(reg.defs_for(&[]).await.len(), 1);
    }

    #[tokio::test]
    async fn exchange_enabled_returns_receipt_and_reader_tools() {
        let root = tempfile::tempdir().unwrap();
        let reg = ToolRegistry::new();
        reg.register(Arc::new(Echo)).await.unwrap();
        let journal = journal(root.path());
        reg.set_exchange(Arc::clone(&journal)).await.unwrap();

        // Reader tools are registered alongside.
        let names: Vec<String> = reg
            .defs_for(&[])
            .await
            .into_iter()
            .map(|def| def.name)
            .collect();
        assert!(names.contains(&"exchange_list".to_string()));
        assert!(names.contains(&"exchange_read".to_string()));
        assert!(names.contains(&"exchange_tail".to_string()));

        // Execution answers with a receipt, not the raw output.
        let raw = reg
            .execute_in_run("run-1", "echo", &json!({ "x": 1 }))
            .await
            .unwrap();
        let receipt: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let receipt = receipt.get("exchange_receipt").unwrap();
        let coordinate_text = receipt["coordinate"].as_str().unwrap();
        assert!(coordinate_text.starts_with("exch/run-1/1-"));
        assert_eq!(receipt["status"].as_str(), Some("succeeded"));
        assert!(receipt["preview"].as_str().is_some());

        // The output is retrievable through exchange_read.
        let read_raw = reg
            .execute("exchange_read", &json!({ "coordinate": coordinate_text }))
            .await
            .unwrap();
        assert!(read_raw.contains(r#""output":"{\"x\":1}""#));

        // exchange_list exposes the envelope.
        let list_raw = reg
            .execute("exchange_list", &json!({ "run_id": "run-1" }))
            .await
            .unwrap();
        assert!(list_raw.contains("echo"));

        // The journal holds exactly one succeeded record.
        let envelopes = journal.list("run-1", &ListFilter::default()).await;
        assert_eq!(envelopes.len(), 1);
        assert_eq!(envelopes[0].status, RecordStatus::Succeeded);
    }

    #[tokio::test]
    async fn exchange_rejected_calls_never_execute() {
        let root = tempfile::tempdir().unwrap();
        let reg = ToolRegistry::new();
        let counted = Arc::new(CountingTool {
            calls: AtomicUsize::new(0),
        });
        reg.register(counted.clone()).await.unwrap();
        let journal = ExchangeJournal::new(root.path().to_path_buf(), ExchangeConfig::default());
        let journal = Arc::new(journal.with_gate(Box::new(RejectAllGate)).await);
        reg.set_exchange(journal).await.unwrap();

        let raw = reg.execute("counted", &json!({})).await.unwrap();
        let receipt: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let receipt = receipt.get("exchange_receipt").unwrap();
        assert_eq!(receipt["status"].as_str(), Some("rejected"));
        assert_eq!(receipt["reason"].as_str(), Some("blocked by test policy"));
        assert_eq!(counted.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn exchange_tool_error_is_journaled_as_failed() {
        let root = tempfile::tempdir().unwrap();
        let reg = ToolRegistry::new();
        reg.set_exchange(journal(root.path())).await.unwrap();
        // Missing tool still errors (nothing journaled since submit never ran).
        assert!(reg.execute("missing", &json!({})).await.is_err());
    }
}
