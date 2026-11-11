//! Steward tool registry — LLM-callable tools for the steward agent loop.
//!
//! Design §2.1.3.2: built-in tools are thin wrappers over existing services
//! (snapshot / config_change / cycle / schedules repo) with zero new business
//! logic. Config-proposal tools are marked `suspend_on_call`: once they
//! produce a proposal, the agent loop suspends and awaits user confirmation
//! over IPC instead of feeding the result back to the model.

use std::sync::atomic::{AtomicU32, Ordering};
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

/// `app_settings` key: max `create_background_task` calls per agent-loop
/// run (default 5, key documented only — no UI, ADR 0029).
pub const MAX_TASKS_PER_LOOP_KEY: &str = "steward.tools.max_tasks_per_loop";
/// Default per-loop task-creation budget (ADR 0029 — aligns with the idle
/// backlog scan's "take first 5" semantics).
pub const DEFAULT_MAX_TASKS_PER_LOOP: u32 = 5;
/// Proposal `source` value for steward-produced dispatch proposals.
pub const PROPOSAL_SOURCE: &str = "steward";
/// Member cap handed to the team planner — mirrors the shell's
/// `FORM_TEAM_MAX_MEMBERS`.
const MAX_PLAN_MEMBERS: usize = 5;
/// Default number of tasks returned by `list_tasks`.
const TASKS_DEFAULT_LIMIT: u32 = 20;
/// Hard cap for the `limit` argument of `list_tasks`.
const TASKS_MAX_LIMIT: u32 = 100;
/// Scan bound for the workspace task list before in-memory filtering.
const TASKS_SCAN_LIMIT: u32 = 200;

/// Execution context handed to every steward tool invocation. Cheap to
/// clone; built once per agent-loop run and shared by all tools.
#[derive(Clone)]
pub struct StewardToolContext {
    pub db_path: Arc<str>,
    pub secrets: Arc<dyn SecretStore>,
    pub session_id: String,
    pub workspace_id: String,
    pub bus: Option<EventBus>,
    /// Per-loop budget for `create_background_task` (ADR 0029): shared by
    /// all clones, reset implicitly by a new loop run building a fresh
    /// context.
    pub task_budget: Arc<AtomicU32>,
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

    /// A registry pre-loaded with the built-in tools.
    pub fn with_builtin_tools() -> Self {
        let mut reg = Self::new();
        let builtins: Vec<Arc<dyn StewardTool>> = vec![
            Arc::new(ReadAppStateTool),
            Arc::new(ProposeConfigChangeTool),
            Arc::new(TriggerEvolutionTool),
            Arc::new(ListSchedulesTool),
            Arc::new(CreateBackgroundTaskTool),
            Arc::new(ProposeTeamDispatchTool),
            Arc::new(DispatchTaskNowTool),
            Arc::new(ListTasksTool),
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

/// `create_background_task` — registers one backlog card on the Kanban
/// board (ADR 0029). Bounded per loop run by `steward.tools.max_tasks_per_loop`.
struct CreateBackgroundTaskTool;

/// Reads the per-loop task-creation budget from `app_settings` (missing or
/// invalid falls back to the default).
fn load_max_tasks_per_loop(conn: &rusqlite::Connection) -> u32 {
    crate::store::repos::settings::get(conn, MAX_TASKS_PER_LOOP_KEY)
        .ok()
        .flatten()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_MAX_TASKS_PER_LOOP)
}

#[async_trait]
impl StewardTool for CreateBackgroundTaskTool {
    fn name(&self) -> &'static str {
        "create_background_task"
    }

    fn description(&self) -> &'static str {
        "Register one background task as a backlog card on the Kanban board \
         of the current workspace. The card is visible and deletable in the \
         Board UI immediately; it does not run anything by itself. Bounded \
         per conversation turn."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Short task title shown on the card."
                },
                "description": {
                    "type": "string",
                    "default": "",
                    "description": "Detailed instructions for the task."
                }
            },
            "required": ["title"],
            "additionalProperties": false,
        })
    }

    async fn execute(
        &self,
        ctx: &StewardToolContext,
        arguments: &serde_json::Value,
    ) -> Result<ToolOutcome, StewardError> {
        let title = arguments
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| StewardError::Intent("missing or empty 'title'".into()))?
            .to_string();
        let description = arguments
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let spent = ctx.task_budget.fetch_add(1, Ordering::Relaxed);
        let db_path = Arc::clone(&ctx.db_path);
        let limit = spawn_blocking(move || -> Result<u32, StewardError> {
            let db = Db::open(&db_path)?;
            Ok(load_max_tasks_per_loop(&db.0))
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))??;
        if spent >= limit {
            return Err(StewardError::Intent(format!(
                "task creation budget for this turn exhausted ({limit}); finish or dispatch existing tasks first"
            )));
        }

        let db_path = Arc::clone(&ctx.db_path);
        let session_id = ctx.session_id.clone();
        let workspace_id = ctx.workspace_id.clone();
        let task = spawn_blocking(move || -> Result<crate::domain::Task, StewardError> {
            let db = Db::open(&db_path)?;
            crate::store::migrations::run(&db.0)?;
            let now = crate::domain::now_ms();
            let task = crate::domain::Task {
                id: crate::domain::new_id(),
                session_id: None,
                title,
                description,
                status: crate::domain::TaskStatus::Backlog,
                created_at: now,
                updated_at: now,
                workspace_id,
                source: PROPOSAL_SOURCE.into(),
                assignee_team_id: None,
                created_by: Some(session_id),
            };
            crate::store::repos::tasks_runs::insert_task(&db.0, &task)?;
            Ok(task)
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))??;
        Ok(ToolOutcome::Result(json!({
            "task_id": task.id,
            "status": task.status.as_str(),
            "workspace_id": task.workspace_id,
        })))
    }
}

/// `propose_team_dispatch` — plans a fresh team for the given tasks and
/// records a pending proposal in the approvals inbox (ADR 0023 pipeline,
/// source='steward'). Suspend-type: the turn ends with the proposal id.
struct ProposeTeamDispatchTool;

/// Loads and validates the dispatch target tasks for a proposal/direct
/// dispatch: exists, belongs to the steward's workspace, startable
/// (backlog/queued). Returns them in the caller's order, deduplicated.
fn load_dispatch_targets(
    conn: &rusqlite::Connection,
    task_ids: &[String],
    workspace_id: &str,
) -> Result<Vec<crate::domain::Task>, StewardError> {
    use crate::store::repos::tasks_runs;
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(task_ids.len());
    for id in task_ids {
        if !seen.insert(id.as_str()) {
            continue;
        }
        let task = tasks_runs::get_task(conn, id).map_err(StewardError::from)?;
        if task.workspace_id != workspace_id {
            return Err(StewardError::Intent(format!(
                "task {id} belongs to another workspace"
            )));
        }
        if !matches!(
            task.status,
            crate::domain::TaskStatus::Backlog | crate::domain::TaskStatus::Queued
        ) {
            return Err(StewardError::Intent(format!(
                "task {id} is '{}' (only backlog/queued tasks can be dispatched)",
                task.status.as_str()
            )));
        }
        out.push(task);
    }
    Ok(out)
}

/// Planning input for a dispatch batch: title plus description when present
/// (the schedule_dispatcher prompt composition semantics).
fn dispatch_prompt(tasks: &[crate::domain::Task]) -> String {
    let mut out = String::from("完成以下后台任务：");
    for task in tasks {
        out.push_str("\n- ");
        out.push_str(&task.title);
        let description = task.description.trim();
        if !description.is_empty() {
            out.push('：');
            out.push_str(description);
        }
    }
    out
}

#[async_trait]
impl StewardTool for ProposeTeamDispatchTool {
    fn name(&self) -> &'static str {
        "propose_team_dispatch"
    }

    fn description(&self) -> &'static str {
        "Plan a new team for the given tasks and record a pending proposal \
         in the approvals inbox. Nothing runs until the user approves the \
         proposal. Use this when no specific team was named; when the user \
         names an existing team, use dispatch_task_now instead."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_ids": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1,
                    "description": "Ids of the backlog/queued tasks to dispatch."
                },
                "preferences": {
                    "type": "string",
                    "description": "Optional hints about the desired team composition."
                }
            },
            "required": ["task_ids"],
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
        let task_ids: Vec<String> = arguments
            .get("task_ids")
            .and_then(|v| v.as_array())
            .filter(|a| !a.is_empty())
            .ok_or_else(|| StewardError::Intent("missing or empty 'task_ids'".into()))?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| StewardError::Intent("'task_ids' must be strings".into()))
            })
            .collect::<Result<_, _>>()?;
        let preferences = arguments
            .get("preferences")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let db_path = Arc::clone(&ctx.db_path);
        let workspace_id = ctx.workspace_id.clone();
        let tasks = spawn_blocking(move || -> Result<Vec<crate::domain::Task>, StewardError> {
            let db = Db::open(&db_path)?;
            load_dispatch_targets(&db.0, &task_ids, &workspace_id)
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))??;

        let mut prompt = dispatch_prompt(&tasks);
        if !preferences.trim().is_empty() {
            prompt.push_str("\n组队偏好：");
            prompt.push_str(preferences.trim());
        }
        let plan_ids = tasks.iter().map(|t| t.id.clone()).collect();
        let draft = crate::services::draft_team_plan(
            Arc::clone(&ctx.db_path),
            Arc::clone(&ctx.secrets),
            None,
            &prompt,
            plan_ids,
            MAX_PLAN_MEMBERS,
        )
        .await
        .map_err(|e| StewardError::Store(e.to_string()))?;
        let plan_json = serde_json::to_string(&draft)?;

        let db_path = Arc::clone(&ctx.db_path);
        let workspace_id = ctx.workspace_id.clone();
        let proposal_id = spawn_blocking(move || -> Result<String, StewardError> {
            let db = Db::open(&db_path)?;
            crate::store::migrations::run(&db.0)?;
            let proposal = crate::domain::Proposal {
                id: crate::domain::new_id(),
                kind: "team_plan".into(),
                source: PROPOSAL_SOURCE.into(),
                plan_json,
                status: crate::domain::ProposalStatus::Pending,
                workspace_id,
                resolved_at: None,
                created_at: crate::domain::now_ms(),
            };
            let id = proposal.id.clone();
            crate::store::repos::proposals::insert(&db.0, &proposal)?;
            Ok(id)
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))??;
        Ok(ToolOutcome::Suspend(StewardReply::TeamProposalPending {
            proposal_id,
            confirm_handle: "resolve_proposal".into(),
        }))
    }
}

/// `dispatch_task_now` — direct dispatch of one task onto a named existing
/// team (ADR 0029). Suspend-type: the turn ends with an in-chat
/// confirmation request; only the confirm IPC starts the run.
struct DispatchTaskNowTool;

#[async_trait]
impl StewardTool for DispatchTaskNowTool {
    fn name(&self) -> &'static str {
        "dispatch_task_now"
    }

    fn description(&self) -> &'static str {
        "Dispatch one backlog/queued task onto an existing team the user \
         named. Produces an in-chat confirmation request; the team run only \
         starts after the user confirms."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "Id of the task to dispatch."
                },
                "team_id": {
                    "type": "string",
                    "description": "Id of the existing team to run the task."
                }
            },
            "required": ["task_id", "team_id"],
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
        let task_id = arguments
            .get("task_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| StewardError::Intent("missing or empty 'task_id'".into()))?
            .to_string();
        let team_id = arguments
            .get("team_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| StewardError::Intent("missing or empty 'team_id'".into()))?
            .to_string();

        let db_path = Arc::clone(&ctx.db_path);
        let workspace_id = ctx.workspace_id.clone();
        let check_task = task_id.clone();
        let check_team = team_id.clone();
        spawn_blocking(move || -> Result<(), StewardError> {
            let db = Db::open(&db_path)?;
            load_dispatch_targets(&db.0, std::slice::from_ref(&check_task), &workspace_id)?;
            crate::store::repos::teams::get(&db.0, &check_team).map_err(StewardError::from)?;
            Ok(())
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))??;
        Ok(ToolOutcome::Suspend(StewardReply::TaskDispatchPending {
            task_id,
            team_id,
            confirm_handle: "steward_confirm_task_dispatch".into(),
        }))
    }
}

/// `list_tasks` — wraps `store::repos::tasks_runs::list_tasks_by_workspace`
/// with in-memory status filtering (the steward's Board view).
struct ListTasksTool;

#[async_trait]
impl StewardTool for ListTasksTool {
    fn name(&self) -> &'static str {
        "list_tasks"
    }

    fn description(&self) -> &'static str {
        "List Kanban tasks of the current workspace, newest first, with \
         their status, source, assignee team and creation time."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["backlog", "queued", "running", "done", "cancelled", "failed"],
                    "description": "Only return tasks with this status."
                },
                "limit": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": TASKS_MAX_LIMIT,
                    "default": TASKS_DEFAULT_LIMIT,
                    "description": "Maximum number of tasks to return."
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
        let status = arguments
            .get("status")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let limit = match arguments.get("limit") {
            None => TASKS_DEFAULT_LIMIT,
            Some(v) => {
                let n = v.as_u64().ok_or_else(|| {
                    StewardError::Intent("'limit' must be a positive integer".into())
                })?;
                u32::try_from(n)
                    .map(|n| n.clamp(1, TASKS_MAX_LIMIT))
                    .map_err(|_| {
                        StewardError::Intent("'limit' must be a positive integer".into())
                    })?
            }
        };
        let db_path = Arc::clone(&ctx.db_path);
        let workspace_id = ctx.workspace_id.clone();
        let tasks = spawn_blocking(move || -> Result<Vec<crate::domain::Task>, StewardError> {
            let db = Db::open(&db_path)?;
            let scanned = crate::store::repos::tasks_runs::list_tasks_by_workspace(
                &db.0,
                &workspace_id,
                TASKS_SCAN_LIMIT,
            )?;
            Ok(match status {
                Some(s) => scanned
                    .into_iter()
                    .filter(|t| t.status.as_str() == s.as_str())
                    .collect(),
                None => scanned,
            })
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))??;
        let items: Vec<serde_json::Value> = tasks
            .into_iter()
            .take(limit as usize)
            .map(|t| {
                json!({
                    "id": t.id,
                    "title": t.title,
                    "status": t.status.as_str(),
                    "source": t.source,
                    "assignee_team_id": t.assignee_team_id,
                    "created_at": t.created_at,
                })
            })
            .collect();
        Ok(ToolOutcome::Result(json!(items)))
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
            task_budget: Arc::new(AtomicU32::new(0)),
        }
    }

    #[test]
    fn builtin_registry_has_eight_tools_in_order() {
        let reg = StewardToolRegistry::with_builtin_tools();
        assert_eq!(
            reg.names(),
            vec![
                "read_app_state",
                "propose_config_change",
                "trigger_evolution",
                "list_schedules",
                "create_background_task",
                "propose_team_dispatch",
                "dispatch_task_now",
                "list_tasks",
            ]
        );
    }

    #[test]
    fn only_suspend_type_tools_suspend() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let suspend_names = [
            "propose_config_change",
            "propose_team_dispatch",
            "dispatch_task_now",
        ];
        for name in reg.names() {
            let tool = reg.get(name).unwrap();
            let expected = suspend_names.contains(&name);
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
        assert_eq!(defs.len(), 8);
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

    fn seed_task(
        conn: &rusqlite::Connection,
        id: &str,
        status: crate::domain::TaskStatus,
        ws: &str,
    ) {
        crate::store::repos::tasks_runs::insert_task(
            conn,
            &crate::domain::Task {
                id: id.into(),
                session_id: None,
                title: format!("task {id}"),
                description: "seeded".into(),
                status,
                created_at: 1,
                updated_at: 1,
                workspace_id: ws.into(),
                source: "manual".into(),
                assignee_team_id: None,
                created_by: None,
            },
        )
        .unwrap();
    }

    fn seed_team(conn: &rusqlite::Connection, id: &str) {
        crate::store::repos::teams::insert(
            conn,
            &crate::domain::Team {
                id: id.into(),
                name: format!("team {id}"),
                topology: crate::domain::TeamTopology::Pipeline,
                member_role_ids: vec![],
                config: serde_json::json!({}),
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
    }

    #[tokio::test]
    async fn create_background_task_persists_steward_card() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let dbp = db_path().await;
        let out = reg
            .execute(
                "create_background_task",
                &ctx(Arc::clone(&dbp)),
                &json!({"title": "整理看板", "description": "清理 done 卡片"}),
            )
            .await
            .unwrap();
        let task_id = match out {
            ToolOutcome::Result(v) => {
                assert_eq!(v.get("status").and_then(|s| s.as_str()), Some("backlog"));
                v.get("task_id")
                    .and_then(|i| i.as_str())
                    .unwrap()
                    .to_string()
            }
            other => panic!("expected Result, got {other:?}"),
        };
        let db = Db::open(&dbp).unwrap();
        let task = crate::store::repos::tasks_runs::get_task(&db.0, &task_id).unwrap();
        assert_eq!(task.title, "整理看板");
        assert_eq!(task.description, "清理 done 卡片");
        assert_eq!(task.status, crate::domain::TaskStatus::Backlog);
        assert_eq!(task.source, "steward");
        assert_eq!(task.workspace_id, "ws-1");
        assert_eq!(task.created_by.as_deref(), Some("sess-1"));
        assert_eq!(task.session_id, None);
    }

    #[tokio::test]
    async fn create_background_task_rejects_empty_title() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let err = reg
            .execute(
                "create_background_task",
                &ctx(db_path().await),
                &json!({"title": "   "}),
            )
            .await
            .expect_err("blank title must be rejected");
        assert!(matches!(err, StewardError::Intent(_)));
    }

    #[tokio::test]
    async fn create_background_task_budget_exhausts_per_loop() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let dbp = db_path().await;
        {
            let db = Db::open(&dbp).unwrap();
            crate::store::repos::settings::set(&db.0, MAX_TASKS_PER_LOOP_KEY, "2").unwrap();
        }
        let c = ctx(Arc::clone(&dbp));
        for i in 0..2 {
            let out = reg
                .execute(
                    "create_background_task",
                    &c,
                    &json!({"title": format!("t{i}")}),
                )
                .await;
            assert!(out.is_ok(), "call {i} within budget must succeed");
        }
        let err = reg
            .execute("create_background_task", &c, &json!({"title": "t3"}))
            .await
            .expect_err("third call must exceed the budget");
        assert!(matches!(err, StewardError::Intent(_)));

        // A fresh loop run gets a fresh budget (new context, same db).
        let fresh = ctx(Arc::clone(&dbp));
        let out = reg
            .execute("create_background_task", &fresh, &json!({"title": "t3"}))
            .await;
        assert!(out.is_ok(), "fresh context resets the budget");
    }

    #[tokio::test]
    async fn dispatch_task_now_suspends_with_pending_reply() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let dbp = db_path().await;
        {
            let db = Db::open(&dbp).unwrap();
            seed_task(&db.0, "t1", crate::domain::TaskStatus::Backlog, "ws-1");
            seed_team(&db.0, "team-1");
        }
        let out = reg
            .execute(
                "dispatch_task_now",
                &ctx(dbp),
                &json!({"task_id": "t1", "team_id": "team-1"}),
            )
            .await
            .unwrap();
        match out {
            ToolOutcome::Suspend(StewardReply::TaskDispatchPending {
                task_id,
                team_id,
                confirm_handle,
            }) => {
                assert_eq!(task_id, "t1");
                assert_eq!(team_id, "team-1");
                assert_eq!(confirm_handle, "steward_confirm_task_dispatch");
            }
            other => panic!("expected Suspend(TaskDispatchPending), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn dispatch_task_now_rejects_invalid_targets() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let dbp = db_path().await;
        {
            let db = Db::open(&dbp).unwrap();
            seed_task(&db.0, "t-done", crate::domain::TaskStatus::Done, "ws-1");
            seed_task(&db.0, "t-other", crate::domain::TaskStatus::Backlog, "ws-2");
            seed_team(&db.0, "team-1");
        }
        let c = ctx(Arc::clone(&dbp));

        let err = reg
            .execute(
                "dispatch_task_now",
                &c,
                &json!({"task_id": "ghost", "team_id": "team-1"}),
            )
            .await
            .expect_err("missing task must be rejected");
        assert!(err.to_string().contains("ghost"));

        let err = reg
            .execute(
                "dispatch_task_now",
                &c,
                &json!({"task_id": "t-done", "team_id": "team-1"}),
            )
            .await
            .expect_err("done task must be rejected");
        assert!(matches!(err, StewardError::Intent(_)));

        let err = reg
            .execute(
                "dispatch_task_now",
                &c,
                &json!({"task_id": "t-other", "team_id": "team-1"}),
            )
            .await
            .expect_err("cross-workspace task must be rejected");
        assert!(matches!(err, StewardError::Intent(_)));

        reg.execute(
            "dispatch_task_now",
            &c,
            &json!({"task_id": "t-other", "team_id": "ghost-team"}),
        )
        .await
        .expect_err("missing team must be rejected");
    }

    #[tokio::test]
    async fn propose_team_dispatch_validates_targets() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let dbp = db_path().await;
        {
            let db = Db::open(&dbp).unwrap();
            seed_task(&db.0, "t1", crate::domain::TaskStatus::Backlog, "ws-1");
        }
        let c = ctx(Arc::clone(&dbp));

        let err = reg
            .execute("propose_team_dispatch", &c, &json!({"task_ids": []}))
            .await
            .expect_err("empty task_ids must be rejected");
        assert!(matches!(err, StewardError::Intent(_)));

        let err = reg
            .execute("propose_team_dispatch", &c, &json!({"task_ids": ["ghost"]}))
            .await
            .expect_err("missing task must be rejected");
        assert!(err.to_string().contains("ghost"));
    }

    #[tokio::test]
    async fn list_tasks_filters_by_status_and_workspace() {
        let reg = StewardToolRegistry::with_builtin_tools();
        let dbp = db_path().await;
        {
            let db = Db::open(&dbp).unwrap();
            seed_task(
                &db.0,
                "t-backlog",
                crate::domain::TaskStatus::Backlog,
                "ws-1",
            );
            seed_task(&db.0, "t-done", crate::domain::TaskStatus::Done, "ws-1");
            seed_task(
                &db.0,
                "t-elsewhere",
                crate::domain::TaskStatus::Backlog,
                "ws-2",
            );
        }
        let out = reg
            .execute("list_tasks", &ctx(Arc::clone(&dbp)), &json!({}))
            .await
            .unwrap();
        match out {
            ToolOutcome::Result(v) => {
                let items = v.as_array().unwrap();
                assert_eq!(items.len(), 2, "ws-2 task must be excluded");
                assert!(items
                    .iter()
                    .any(|i| i.get("id").and_then(|x| x.as_str()) == Some("t-backlog")));
            }
            other => panic!("expected Result, got {other:?}"),
        }

        let out = reg
            .execute("list_tasks", &ctx(dbp), &json!({"status": "done"}))
            .await
            .unwrap();
        match out {
            ToolOutcome::Result(v) => {
                let items = v.as_array().unwrap();
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].get("id").and_then(|x| x.as_str()), Some("t-done"));
            }
            other => panic!("expected Result, got {other:?}"),
        }
    }
}
