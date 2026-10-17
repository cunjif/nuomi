//! Steward tool registry — LLM-callable tools for the steward agent loop.
//!
//! Design §2.1.3.2: built-in tools are thin wrappers over existing services
//! (snapshot / config_change / cycle / schedules repo) with zero new business
//! logic. Config-proposal tools are marked `suspend_on_call`: once they
//! produce a proposal, the agent loop suspends and awaits user confirmation
//! over IPC instead of feeding the result back to the model.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use tokio::task::spawn_blocking;

use crate::harness::EventBus;
use crate::providers::{SecretStore, ToolDef};
use crate::store::Db;

use super::config_change::{self, ConfigTarget};
use super::cycle::{self, EvolutionTrigger};
use super::kernel::StewardReply;
use super::snapshot;
use super::StewardError;

/// Default number of schedules returned by `list_schedules`.
const SCHEDULES_DEFAULT_LIMIT: u32 = 20;
/// Hard cap for the `limit` argument of `list_schedules`.
const SCHEDULES_MAX_LIMIT: u32 = 100;

/// Execution context handed to every steward tool invocation. Cheap to
/// clone; built once per agent-loop run and shared by all tools.
#[derive(Clone)]
pub struct StewardToolContext {
    pub db_path: Arc<str>,
    pub secrets: Arc<dyn SecretStore>,
    pub session_id: String,
    pub workspace_id: String,
    pub bus: Option<EventBus>,
}

/// Outcome of a steward tool execution.
#[derive(Debug)]
pub enum ToolOutcome {
    /// Normal result — serialized JSON fed back to the model as the tool
    /// result message; the loop continues.
    Result(serde_json::Value),
    /// Suspend-type outcome — the tool produced a user-facing proposal; the
    /// agent loop must stop and return the embedded reply (SuspendCheck,
    /// design §2.1.3.1).
    Suspend(StewardReply),
}

/// An LLM-callable steward tool (design §2.1.3.2).
#[async_trait]
pub trait StewardTool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// JSON-Schema object describing the tool's parameters.
    fn parameters_schema(&self) -> serde_json::Value;
    /// Config-proposal tools return `true`: the loop suspends after the call
    /// and awaits user confirmation instead of feeding the result back.
    fn suspend_on_call(&self) -> bool {
        false
    }
    async fn execute(
        &self,
        ctx: &StewardToolContext,
        arguments: &serde_json::Value,
    ) -> Result<ToolOutcome, StewardError>;
}

/// Serializes a tool definition into the providers-layer `ToolDef` wire type.
fn tool_def(tool: &dyn StewardTool) -> ToolDef {
    ToolDef {
        name: tool.name().to_string(),
        description: tool.description().to_string(),
        parameters: tool.parameters_schema(),
    }
}

/// Parses a `ConfigTarget` from `{"target_type", "target_id"}` tool arguments.
/// Mirrors the mapping in `kernel::ConfigChangeRequest::to_target`.
fn parse_config_target(args: &serde_json::Value) -> Result<ConfigTarget, StewardError> {
    let target_type = args
        .get("target_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            StewardError::Intent(
                "missing or invalid 'target_type' (provider|role|team|agent_profile)".into(),
            )
        })?;
    let target_id = args
        .get("target_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| StewardError::Intent("missing or invalid 'target_id'".into()))?
        .to_string();
    match target_type {
        "provider" => Ok(ConfigTarget::Provider { id: target_id }),
        "role" => Ok(ConfigTarget::Role { id: target_id }),
        "team" => Ok(ConfigTarget::Team { id: target_id }),
        "agent_profile" => Ok(ConfigTarget::AgentProfile { id: target_id }),
        other => Err(StewardError::Intent(format!(
            "unknown target_type: {other}"
        ))),
    }
}

// ------------------------------------------------------------------ registry

/// Registry of steward tools: registration + dispatch by name.
#[derive(Default)]
pub struct StewardToolRegistry {
    tools: Vec<Arc<dyn StewardTool>>,
}

impl StewardToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// A registry pre-loaded with the four built-in tools.
    pub fn with_builtin_tools() -> Self {
        let mut reg = Self::new();
        let builtins: Vec<Arc<dyn StewardTool>> = vec![
            Arc::new(ReadAppStateTool),
            Arc::new(ProposeConfigChangeTool),
            Arc::new(TriggerEvolutionTool),
            Arc::new(ListSchedulesTool),
        ];
        for tool in builtins {
            if let Err(e) = reg.register(tool) {
                tracing::warn!(error = %e, "duplicate builtin steward tool name");
            }
        }
        reg
    }

    /// Registers a tool; rejects duplicate names.
    pub fn register(&mut self, tool: Arc<dyn StewardTool>) -> Result<(), StewardError> {
        if self.tools.iter().any(|t| t.name() == tool.name()) {
            return Err(StewardError::Conflict {
                entity: "steward_tool",
                id: tool.name().to_string(),
                reason: "tool already registered".into(),
            });
        }
        self.tools.push(tool);
        Ok(())
    }

    /// Looks a tool up by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn StewardTool>> {
        self.tools.iter().find(|t| t.name() == name).cloned()
    }

    /// Serializes all registered tool definitions for a `ChatRequest`.
    pub fn tool_defs(&self) -> Vec<ToolDef> {
        self.tools.iter().map(|t| tool_def(t.as_ref())).collect()
    }

    /// Registered tool names, in registration order.
    pub fn names(&self) -> Vec<&'static str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    /// Dispatches an execution by tool name. Unknown names yield
    /// `StewardError::NotFound`.
    pub async fn execute(
        &self,
        name: &str,
        ctx: &StewardToolContext,
        arguments: &serde_json::Value,
    ) -> Result<ToolOutcome, StewardError> {
        let tool = self.get(name).ok_or_else(|| StewardError::NotFound {
            entity: "steward_tool",
            id: name.to_string(),
        })?;
        tool.execute(ctx, arguments).await
    }
}

// ------------------------------------------------------- built-in tools

/// `read_app_state` — wraps [`snapshot::read`]; hands the model a
/// point-in-time app-state snapshot to reason over.
struct ReadAppStateTool;

#[async_trait]
impl StewardTool for ReadAppStateTool {
    fn name(&self) -> &'static str {
        "read_app_state"
    }

    fn description(&self) -> &'static str {
        "Read a point-in-time snapshot of the application state: providers, \
         roles, teams, agent profiles, sessions, recent events, memory \
         entries, prompt versions and evolution cycles."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false,
        })
    }

    async fn execute(
        &self,
        ctx: &StewardToolContext,
        _arguments: &serde_json::Value,
    ) -> Result<ToolOutcome, StewardError> {
        let snap = snapshot::read(Arc::clone(&ctx.db_path), &ctx.workspace_id).await?;
        Ok(ToolOutcome::Result(serde_json::to_value(&snap)?))
    }
}

/// `propose_config_change` — wraps [`config_change::propose`].
/// Suspend-type: producing the proposal ends the loop turn with a
/// `ConfigProposal` reply awaiting user confirmation.
struct ProposeConfigChangeTool;

#[async_trait]
impl StewardTool for ProposeConfigChangeTool {
    fn name(&self) -> &'static str {
        "propose_config_change"
    }

    fn description(&self) -> &'static str {
        "Propose a configuration change (partial JSON patch merged shallowly \
         into the current config) for a provider, role, team or agent \
         profile. Produces a diff preview that the user must confirm before \
         it takes effect."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "target_type": {
                    "type": "string",
                    "enum": ["provider", "role", "team", "agent_profile"],
                    "description": "Which kind of config entity to change."
                },
                "target_id": {
                    "type": "string",
                    "description": "Id of the target entity."
                },
                "patch": {
                    "type": "object",
                    "description": "Partial JSON object merged (shallowly) into the current config."
                }
            },
            "required": ["target_type", "target_id", "patch"],
            "additionalProperties": false,
        })
    }

    fn suspend_on_call(&self) -> bool {
        true
    }

    async fn execute(
        &self,
        ctx: &StewardToolContext,
        arguments: &serde_json::Value,
    ) -> Result<ToolOutcome, StewardError> {
        let target = parse_config_target(arguments)?;
        let patch = arguments
            .get("patch")
            .cloned()
            .ok_or_else(|| StewardError::Intent("missing 'patch' object".into()))?;
        if !patch.is_object() {
            return Err(StewardError::Intent("'patch' must be a JSON object".into()));
        }
        let intent = serde_json::to_string(&patch)?;
        let proposal = config_change::propose(Arc::clone(&ctx.db_path), target, &intent).await?;
        Ok(ToolOutcome::Suspend(StewardReply::ConfigProposal {
            proposal_id: proposal.proposal_id,
            diff: proposal.diff_preview,
            confirm_handle: "steward_confirm_config_change".into(),
        }))
    }
}

/// `trigger_evolution` — wraps [`cycle::trigger_evolution`]; accepts a
/// natural-language instruction, returns immediately with the accepted cycle.
struct TriggerEvolutionTool;

#[async_trait]
impl StewardTool for TriggerEvolutionTool {
    fn name(&self) -> &'static str {
        "trigger_evolution"
    }

    fn description(&self) -> &'static str {
        "Trigger a new self-evolution cycle with a natural-language \
         instruction describing what should evolve. Returns immediately with \
         the accepted cycle id and status; the actual work runs in the \
         background."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "instruction": {
                    "type": "string",
                    "description": "What the evolution cycle should work on."
                }
            },
            "required": ["instruction"],
            "additionalProperties": false,
        })
    }

    async fn execute(
        &self,
        ctx: &StewardToolContext,
        arguments: &serde_json::Value,
    ) -> Result<ToolOutcome, StewardError> {
        let instruction = arguments
            .get("instruction")
            .and_then(|v| v.as_str())
            .ok_or_else(|| StewardError::Intent("missing or invalid 'instruction'".into()))?
            .to_string();
        let trigger = EvolutionTrigger::UserExplicit {
            session_id: ctx.session_id.clone(),
            instruction,
        };
        let accepted = cycle::trigger_evolution(
            Arc::clone(&ctx.db_path),
            Arc::clone(&ctx.secrets),
            trigger,
            ctx.bus.clone(),
        )
        .await?;
        Ok(ToolOutcome::Result(json!({
            "cycle_id": accepted.id,
            "phase": accepted.phase.as_str(),
            "status": accepted.status.as_str(),
        })))
    }
}

/// `list_schedules` — wraps `store::repos::tasks_runs::list_schedules`.
struct ListSchedulesTool;

#[async_trait]
impl StewardTool for ListSchedulesTool {
    fn name(&self) -> &'static str {
        "list_schedules"
    }

    fn description(&self) -> &'static str {
        "List configured schedules (cron/interval triggers), newest first, \
         including their cron expression, enabled flag and next trigger time."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": SCHEDULES_MAX_LIMIT,
                    "default": SCHEDULES_DEFAULT_LIMIT,
                    "description": "Maximum number of schedules to return."
                }
            },
            "additionalProperties": false,
        })
    }

    async fn execute(
        &self,
        ctx: &StewardToolContext,
        arguments: &serde_json::Value,
    ) -> Result<ToolOutcome, StewardError> {
        let limit = match arguments.get("limit") {
            None => SCHEDULES_DEFAULT_LIMIT,
            Some(v) => {
                let n = v.as_u64().ok_or_else(|| {
                    StewardError::Intent("'limit' must be a positive integer".into())
                })?;
                u32::try_from(n)
                    .map(|n| n.clamp(1, SCHEDULES_MAX_LIMIT))
                    .map_err(|_| {
                        StewardError::Intent("'limit' must be a positive integer".into())
                    })?
            }
        };
        let db_path = Arc::clone(&ctx.db_path);
        let schedules = spawn_blocking(move || {
            let db = Db::open(&db_path)?;
            crate::store::repos::tasks_runs::list_schedules(&db.0, limit)
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))??;
        Ok(ToolOutcome::Result(serde_json::to_value(&schedules)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubEchoTool;

    #[async_trait]
    impl StewardTool for StubEchoTool {
        fn name(&self) -> &'static str {
            "stub_echo"
        }
        fn description(&self) -> &'static str {
            "test stub"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            json!({"type": "object", "properties": {"echo": {"type": "string"}}})
        }
        async fn execute(
            &self,
            _ctx: &StewardToolContext,
            arguments: &serde_json::Value,
        ) -> Result<ToolOutcome, StewardError> {
            Ok(ToolOutcome::Result(json!({
                "echoed": arguments.get("echo").cloned().unwrap_or(serde_json::Value::Null),
            })))
        }
    }

    async fn db_path() -> Arc<str> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_string_lossy().to_string();
        let db = Db::open(&path_str).unwrap();
        crate::store::migrations::run(&db.0).unwrap();
        drop(db);
        std::mem::forget(dir);
        Arc::from(path_str)
    }

    fn ctx(db_path: Arc<str>) -> StewardToolContext {
        StewardToolContext {
            db_path,
            secrets: Arc::new(crate::providers::MemorySecretStore::default()),
            session_id: "sess-1".into(),
            workspace_id: "ws-1".into(),
            bus: None,
        }
    }

    #[test]
    fn builtin_registry_has_four_tools_in_order() {
        let reg = StewardToolRegistry::with_builtin_tools();
        assert_eq!(
            reg.names(),
            vec![
                "read_app_state",
                "propose_config_change",
                "trigger_evolution",
                "list_schedules",
            ]
        );
    }

    #[test]
    fn only_config_proposal_tool_suspends() {
        let reg = StewardToolRegistry::with_builtin_tools();
        for name in reg.names() {
            let tool = reg.get(name).unwrap();
            let expected = name == "propose_config_change";
            assert_eq!(
                tool.suspend_on_call(),
                expected,
                "unexpected suspend_on_call for {name}"
            );
        }
        // Default (non-overridden) is false.
        assert!(!StubEchoTool.suspend_on_call());
    }

    #[test]
    fn tool_defs_serialization_format() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let defs = reg.tool_defs();
        assert_eq!(defs.len(), 4);
        for def in &defs {
            assert!(!def.name.is_empty());
            assert!(!def.description.is_empty());
            assert_eq!(
                def.parameters.get("type").and_then(|t| t.as_str()),
                Some("object")
            );
        }
        let propose = &defs
            .iter()
            .find(|d| d.name == "propose_config_change")
            .unwrap();
        let required = propose
            .parameters
            .get("required")
            .and_then(|r| r.as_array())
            .unwrap();
        assert!(required.iter().any(|v| v == "patch"));
        let target_type = propose
            .parameters
            .pointer("/properties/target_type/enum")
            .and_then(|e| e.as_array())
            .unwrap();
        assert_eq!(target_type.len(), 4);
    }

    #[test]
    fn register_rejects_duplicate_names() {
        let mut reg = StewardToolRegistry::new();
        reg.register(Arc::new(StubEchoTool)).unwrap();
        let err = reg
            .register(Arc::new(StubEchoTool))
            .expect_err("duplicate must be rejected");
        assert!(matches!(err, StewardError::Conflict { .. }));
        assert_eq!(reg.names(), vec!["stub_echo"]);
    }

    #[tokio::test]
    async fn registry_dispatches_by_name() {
        let mut reg = StewardToolRegistry::with_builtin_tools();
        reg.register(Arc::new(StubEchoTool)).unwrap();
        let c = ctx(db_path().await);

        let out = reg
            .execute("stub_echo", &c, &json!({"echo": "hello"}))
            .await
            .unwrap();
        match out {
            ToolOutcome::Result(v) => {
                assert_eq!(v.get("echoed").and_then(|e| e.as_str()), Some("hello"));
            }
            other => panic!("expected Result, got {other:?}"),
        }

        let out = reg.execute("read_app_state", &c, &json!({})).await.unwrap();
        match out {
            ToolOutcome::Result(v) => {
                assert!(v.get("roles").and_then(|r| r.as_array()).is_some());
            }
            other => panic!("expected Result, got {other:?}"),
        }

        let err = reg
            .execute("no_such_tool", &c, &json!({}))
            .await
            .expect_err("unknown tool must be rejected");
        assert!(matches!(err, StewardError::NotFound { .. }));
    }

    #[tokio::test]
    async fn read_app_state_returns_snapshot_json() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let dbp = db_path().await;
        {
            let db = Db::open(&dbp).unwrap();
            db.0
                .execute(
                    "INSERT INTO provider_configs (id, name, protocol, base_url, created_at, updated_at)
                     VALUES ('p1', 'TestProvider', 'openai_compatible', 'http://localhost', 1, 1)",
                    [],
                )
                .unwrap();
        }
        let out = reg
            .execute("read_app_state", &ctx(dbp), &json!({}))
            .await
            .unwrap();
        match out {
            ToolOutcome::Result(v) => {
                let providers = v.get("providers").and_then(|p| p.as_array()).unwrap();
                assert!(providers
                    .iter()
                    .any(|p| p.get("id").and_then(|i| i.as_str()) == Some("p1")));
                assert_eq!(
                    v.get("roles").and_then(|r| r.as_array()).map(|r| r.len()),
                    Some(6),
                    "6 builtin steward roles should be present"
                );
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn propose_config_change_returns_suspend_proposal() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let dbp = db_path().await;
        {
            let db = Db::open(&dbp).unwrap();
            db.0
                .execute(
                    "INSERT INTO provider_configs (id, name, protocol, base_url, created_at, updated_at)
                     VALUES ('p1', 'TestProvider', 'openai_compatible', 'http://localhost', 1, 1)",
                    [],
                )
                .unwrap();
        }
        let args = json!({
            "target_type": "provider",
            "target_id": "p1",
            "patch": {"name": "Renamed"},
        });
        let out = reg
            .execute("propose_config_change", &ctx(dbp), &args)
            .await
            .unwrap();
        match out {
            ToolOutcome::Suspend(StewardReply::ConfigProposal {
                proposal_id,
                diff,
                confirm_handle,
            }) => {
                assert!(!proposal_id.is_empty());
                assert!(
                    diff.contains("Renamed"),
                    "diff should contain the patched name"
                );
                assert_eq!(confirm_handle, "steward_confirm_config_change");
            }
            other => panic!("expected Suspend(ConfigProposal), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn propose_config_change_validates_arguments() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let c = ctx(db_path().await);

        let err = reg
            .execute(
                "propose_config_change",
                &c,
                &json!({"target_type": "provider", "target_id": "p1"}),
            )
            .await
            .expect_err("missing patch must be rejected");
        assert!(matches!(err, StewardError::Intent(_)));

        let err = reg
            .execute(
                "propose_config_change",
                &c,
                &json!({"target_type": "workspace", "target_id": "w1", "patch": {}}),
            )
            .await
            .expect_err("unknown target_type must be rejected");
        assert!(matches!(err, StewardError::Intent(_)));
    }

    #[tokio::test]
    async fn trigger_evolution_requires_instruction() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let err = reg
            .execute("trigger_evolution", &ctx(db_path().await), &json!({}))
            .await
            .expect_err("missing instruction must be rejected");
        assert!(matches!(err, StewardError::Intent(_)));
    }

    #[tokio::test]
    async fn list_schedules_returns_array() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let out = reg
            .execute("list_schedules", &ctx(db_path().await), &json!({}))
            .await
            .unwrap();
        match out {
            ToolOutcome::Result(v) => {
                assert!(
                    v.as_array().is_some(),
                    "snapshot of fresh db has no schedules"
                );
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn list_schedules_validates_limit() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let err = reg
            .execute(
                "list_schedules",
                &ctx(db_path().await),
                &json!({"limit": "many"}),
            )
            .await
            .expect_err("non-numeric limit must be rejected");
        assert!(matches!(err, StewardError::Intent(_)));
    }
}
