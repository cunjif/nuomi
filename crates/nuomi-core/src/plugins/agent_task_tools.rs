//! Agent task dispatch tools for RoleAgent conversations (ADR-0030).
//!
//! Two tools that let a RoleAgent create background tasks from within its
//! conversation loop:
//!
//! - `create_self_task` (Result-type): creates a task + shadow team + auto-run.
//!   No user confirmation needed — the agent clones itself and runs in the
//!   background.
//!
//! - `propose_team_dispatch` (Suspend-type): plans a multi-role team and
//!   records a proposal in the approvals inbox. The loop suspends until the
//!   user approves or denies.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use tokio::task::spawn_blocking;

use crate::domain::{new_id, now_ms, RunEvent, RunState, Task, TaskStatus};
use crate::harness::{EventBus, HarnessError};
use crate::providers::{SecretStore, ToolDef};
use crate::services::shadow_team::create_shadow_team;
use crate::store::{repos, Db};

use super::tools::{SuspendRequest, Tool};

/// Type alias for the shared session injection queue (ADR-0030).
/// Same type as `NuomiKernel::SessionInjectionQueue` but defined here
/// to avoid a circular dependency on the facade.
pub type SessionInjectionQueue =
    Arc<std::sync::Mutex<std::collections::HashMap<String, std::collections::VecDeque<String>>>>;

/// Default per-loop task-creation budget for agent dispatch (ADR-0030).
pub const DEFAULT_MAX_TASKS_PER_LOOP: u32 = 3;
/// `app_settings` key for the per-loop task-creation budget.
pub const MAX_TASKS_PER_LOOP_KEY: &str = "agent.tools.max_tasks_per_loop";
/// Task source value for RoleAgent-created tasks.
pub const TASK_SOURCE: &str = "agent";
/// Proposal source value for RoleAgent-produced dispatch proposals.
pub const PROPOSAL_SOURCE: &str = "agent";
/// Member cap for the team planner.
const MAX_PLAN_MEMBERS: usize = 5;

/// Execution context for agent task dispatch tools. Cheap to clone; built
/// once per RoleAgent conversation turn and shared by both tools.
#[derive(Clone)]
pub struct AgentTaskToolContext {
    pub db_path: Arc<str>,
    pub secrets: Arc<dyn SecretStore>,
    pub session_id: String,
    pub workspace_id: String,
    pub role_id: String,
    pub bus: Option<EventBus>,
    /// Per-loop budget shared between both tools (AtomicU32 for cheap
    /// clone + shared state across tool instances).
    pub task_budget: Arc<AtomicU32>,
    /// Shared injection queue for result callback (ADR-0030). Background
    /// tasks push result summaries here; the kernel drains them into the
    /// parent conversation's transcript on the next turn.
    pub injections: SessionInjectionQueue,
}

/// Reads the per-loop task-creation budget from `app_settings`.
fn load_max_tasks_per_loop(conn: &rusqlite::Connection) -> u32 {
    crate::store::repos::settings::get(conn, MAX_TASKS_PER_LOOP_KEY)
        .ok()
        .flatten()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_MAX_TASKS_PER_LOOP)
}

/// Transitions a run's state, persisting the state_changed event record
/// before the status update (iron rule). Mirrors the shell-layer
/// `transition_run` but in the core layer.
fn transition_run(
    conn: &rusqlite::Connection,
    run_id: &str,
    expected: RunState,
    ev: RunEvent,
) -> Result<RunState, HarnessError> {
    let next = expected
        .transition(ev)
        .map_err(|e| HarnessError::PluginFailed {
            plugin: "agent_task".into(),
            phase: "transition",
            message: e.to_string(),
        })?;
    let now = now_ms();
    let payload = serde_json::json!({ "from": expected.as_str(), "to": next.as_str() });
    repos::events::append(conn, "run", run_id, "state_changed", &payload, now).map_err(|e| {
        HarnessError::PluginFailed {
            plugin: "agent_task".into(),
            phase: "transition",
            message: e.to_string(),
        }
    })?;
    repos::tasks_runs::update_run_status(conn, run_id, expected, next, now).map_err(|e| {
        HarnessError::PluginFailed {
            plugin: "agent_task".into(),
            phase: "transition",
            message: e.to_string(),
        }
    })?;
    Ok(next)
}

// ================================================================ create_self_task

/// `create_self_task` — creates a background task and auto-launches a
/// single-member shadow team to execute it (ADR-0030 type 1).
///
/// Result-type tool: returns `{ task_id, team_id, status: "running" }`
/// immediately; the team run executes in the background.
pub struct CreateSelfTaskTool {
    ctx: Arc<AgentTaskToolContext>,
}

impl CreateSelfTaskTool {
    pub fn new(ctx: Arc<AgentTaskToolContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for CreateSelfTaskTool {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "create_self_task".into(),
            description: "Create a background task and immediately launch a \
                subagent (cloned from your configuration) to execute it. \
                No user confirmation needed. The subagent runs in the \
                background; its result will be injected into this \
                conversation when it finishes."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Short task title."
                    },
                    "description": {
                        "type": "string",
                        "default": "",
                        "description": "Detailed instructions for the task."
                    }
                },
                "required": ["title"],
                "additionalProperties": false,
            }),
        }
    }

    async fn execute(&self, arguments: &serde_json::Value) -> Result<String, HarnessError> {
        let title = arguments
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| HarnessError::PluginFailed {
                plugin: "agent_task".into(),
                phase: "validate",
                message: "missing or empty 'title'".into(),
            })?
            .to_string();
        let description = arguments
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Budget check
        let spent = self.ctx.task_budget.fetch_add(1, Ordering::Relaxed);
        let db_path = Arc::clone(&self.ctx.db_path);
        let limit = spawn_blocking(move || -> Result<u32, HarnessError> {
            let db = Db::open(&db_path)?;
            Ok(load_max_tasks_per_loop(&db.0))
        })
        .await
        .map_err(|e| HarnessError::PluginFailed {
            plugin: "agent_task".into(),
            phase: "budget",
            message: format!("join error: {e}"),
        })??;
        if spent >= limit {
            return Err(HarnessError::PluginFailed {
                plugin: "agent_task".into(),
                phase: "budget",
                message: format!(
                    "task creation budget for this turn exhausted ({limit}); finish or dispatch existing tasks first"
                ),
            });
        }

        // 1. Create the task
        let db_path = Arc::clone(&self.ctx.db_path);
        let session_id = self.ctx.session_id.clone();
        let workspace_id = self.ctx.workspace_id.clone();
        let task = spawn_blocking(move || -> Result<Task, HarnessError> {
            let db = Db::open(&db_path)?;
            crate::store::migrations::run(&db.0)?;
            let now = now_ms();
            let task = Task {
                id: new_id(),
                session_id: None,
                title,
                description,
                status: TaskStatus::Backlog,
                created_at: now,
                updated_at: now,
                workspace_id,
                source: TASK_SOURCE.into(),
                assignee_team_id: None,
                created_by: Some(session_id),
            };
            repos::tasks_runs::insert_task(&db.0, &task)?;
            Ok(task)
        })
        .await
        .map_err(|e| HarnessError::PluginFailed {
            plugin: "agent_task".into(),
            phase: "create_task",
            message: format!("join error: {e}"),
        })??;

        // 2. Create shadow team (clones current role)
        let shadow = create_shadow_team(Arc::clone(&self.ctx.db_path), &self.ctx.role_id)
            .await
            .map_err(|e| HarnessError::PluginFailed {
                plugin: "agent_task".into(),
                phase: "shadow_team",
                message: e.to_string(),
            })?;

        // 3. Transition task Backlog → Queued + backfill assignee_team_id
        let db_path = Arc::clone(&self.ctx.db_path);
        let task_id = task.id.clone();
        let team_id = shadow.team_id.clone();
        spawn_blocking(move || -> Result<(), HarnessError> {
            let db = Db::open(&db_path)?;
            let now = now_ms();
            repos::tasks_runs::update_task_status(
                &db.0,
                &task_id,
                TaskStatus::Backlog,
                TaskStatus::Queued,
                now,
            )?;
            repos::tasks_runs::assign_task_team(&db.0, &task_id, &team_id, now)?;
            Ok(())
        })
        .await
        .map_err(|e| HarnessError::PluginFailed {
            plugin: "agent_task".into(),
            phase: "transition",
            message: format!("join error: {e}"),
        })??;

        // 4. Spawn background team run
        let db_path = Arc::clone(&self.ctx.db_path);
        let secrets = Arc::clone(&self.ctx.secrets);
        let bus = self.ctx.bus.clone();
        let session_id = self.ctx.session_id.clone();
        let workspace_id = self.ctx.workspace_id.clone();
        let team_id_clone = shadow.team_id.clone();
        let task_title = if task.description.is_empty() {
            task.title.clone()
        } else {
            format!("{}\n{}", task.title, task.description)
        };
        let task_id_for_run = task.id.clone();
        let injections = Arc::clone(&self.ctx.injections);
        let task_title_for_msg = task.title.clone();

        tokio::spawn(async move {
            // Create Run record + transition to Running
            let run = {
                let db_path = Arc::clone(&db_path);
                let task_id_clone = task_id_for_run.clone();
                let session_id_clone = session_id.clone();
                spawn_blocking(move || -> Result<crate::domain::Run, HarnessError> {
                    let db = Db::open(&db_path)?;
                    let now = now_ms();
                    let run = crate::domain::Run {
                        id: new_id(),
                        task_id: task_id_clone,
                        session_id: session_id_clone,
                        status: RunState::Queued,
                        heartbeat_at: now,
                        created_at: now,
                        updated_at: now,
                    };
                    repos::tasks_runs::insert_run(&db.0, &run)?;
                    // Iron rule: persist state_changed BEFORE side effects
                    transition_run(&db.0, &run.id, RunState::Queued, RunEvent::Start)?;
                    Ok(run)
                })
                .await
            };

            let run = match run {
                Ok(Ok(r)) => r,
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "failed to create run for self-dispatch");
                    return;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to create run for self-dispatch");
                    return;
                }
            };

            // Execute team run
            let result = crate::services::team_runner::run_team(
                Arc::clone(&db_path),
                bus.clone(),
                &team_id_clone,
                &run.session_id,
                &task_title,
                secrets,
                None,
                Some(workspace_id),
            )
            .await;

            // Settle run + task
            let run_id = run.id.clone();
            let db_path_settle = Arc::clone(&db_path);
            let task_id_settle = task_id_for_run.clone();
            let session_id_settle = session_id.clone();
            let injections_settle = Arc::clone(&injections);
            let task_title_settle = task_title_for_msg.clone();
            spawn_blocking(move || {
                let db = Db::open(&db_path_settle)?;
                let current = repos::tasks_runs::get_run(&db.0, &run_id)?.status;
                match result {
                    Ok(outcome) => {
                        tracing::info!(
                            run_id = %run_id,
                            rounds = outcome.rounds,
                            "self-dispatch team run completed"
                        );
                        transition_run(&db.0, &run_id, current, RunEvent::Succeed)?;
                        let now = now_ms();
                        repos::tasks_runs::update_task_status(
                            &db.0,
                            &task_id_settle,
                            TaskStatus::Queued,
                            TaskStatus::Done,
                            now,
                        )?;
                        // ADR-0030: Push result summary to the parent
                        // conversation's injection queue.
                        let summary = format!(
                            "[后台任务完成] 「{}」已完成。\n结果摘要：{}",
                            task_title_settle,
                            outcome.final_output.chars().take(2000).collect::<String>()
                        );
                        let mut map = injections_settle
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        map.entry(session_id_settle)
                            .or_default()
                            .push_back(summary);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, run_id = %run_id, "self-dispatch team run failed");
                        transition_run(&db.0, &run_id, current, RunEvent::Fail)?;
                        let now = now_ms();
                        repos::tasks_runs::update_task_status(
                            &db.0,
                            &task_id_settle,
                            TaskStatus::Queued,
                            TaskStatus::Failed,
                            now,
                        )?;
                        // ADR-0030: Push failure summary to the parent
                        // conversation's injection queue.
                        let summary = format!(
                            "[后台任务失败] 「{}」执行失败：{}",
                            task_title_settle,
                            e
                        );
                        let mut map = injections_settle
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        map.entry(session_id_settle)
                            .or_default()
                            .push_back(summary);
                    }
                }
                Ok::<(), HarnessError>(())
            })
            .await
            .ok();
        });

        Ok(json!({
            "task_id": task.id,
            "team_id": shadow.team_id,
            "status": "running",
            "message": "Subagent launched in background. The result will appear in this conversation when it completes."
        })
        .to_string())
    }
}

// ================================================================ propose_team_dispatch

/// `propose_team_dispatch` — plans a multi-role team and records a pending
/// proposal in the approvals inbox (ADR-0030 type 2).
///
/// Suspend-type tool: the loop pauses after the proposal is created, waiting
/// for the user to approve or deny in the approvals inbox.
pub struct ProposeTeamDispatchTool {
    ctx: Arc<AgentTaskToolContext>,
}

impl ProposeTeamDispatchTool {
    pub fn new(ctx: Arc<AgentTaskToolContext>) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for ProposeTeamDispatchTool {
    fn def(&self) -> ToolDef {
        ToolDef {
            name: "propose_team_dispatch".into(),
            description: "Plan a new team (with other roles) for the given \
                tasks and record a pending proposal in the approvals inbox. \
                Nothing runs until the user approves the proposal. Use this \
                when you need help from other roles."
                .into(),
            parameters: json!({
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
            }),
        }
    }

    fn suspend_on_call(&self) -> bool {
        true
    }

    async fn execute(&self, arguments: &serde_json::Value) -> Result<String, HarnessError> {
        let task_ids: Vec<String> = arguments
            .get("task_ids")
            .and_then(|v| v.as_array())
            .filter(|a| !a.is_empty())
            .ok_or_else(|| HarnessError::PluginFailed {
                plugin: "agent_task".into(),
                phase: "validate",
                message: "missing or empty 'task_ids'".into(),
            })?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| HarnessError::PluginFailed {
                        plugin: "agent_task".into(),
                        phase: "validate",
                        message: "'task_ids' must be strings".into(),
                    })
            })
            .collect::<Result<_, _>>()?;
        let preferences = arguments
            .get("preferences")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Validate tasks exist and belong to this workspace
        let db_path = Arc::clone(&self.ctx.db_path);
        let workspace_id = self.ctx.workspace_id.clone();
        let tasks = spawn_blocking(move || -> Result<Vec<crate::domain::Task>, HarnessError> {
            let db = Db::open(&db_path)?;
            let mut out = Vec::with_capacity(task_ids.len());
            for id in &task_ids {
                let task = repos::tasks_runs::get_task(&db.0, id)?;
                if task.workspace_id != workspace_id {
                    return Err(HarnessError::PluginFailed {
                        plugin: "agent_task".into(),
                        phase: "validate",
                        message: format!("task {id} belongs to another workspace"),
                    });
                }
                if !matches!(task.status, TaskStatus::Backlog | TaskStatus::Queued) {
                    return Err(HarnessError::PluginFailed {
                        plugin: "agent_task".into(),
                        phase: "validate",
                        message: format!(
                            "task {} is '{}' (only backlog/queued tasks can be dispatched)",
                            id,
                            task.status.as_str()
                        ),
                    });
                }
                out.push(task);
            }
            Ok(out)
        })
        .await
        .map_err(|e| HarnessError::PluginFailed {
            plugin: "agent_task".into(),
            phase: "validate",
            message: format!("join error: {e}"),
        })??;

        // Build planning prompt
        let mut prompt = String::from("完成以下后台任务：");
        for task in &tasks {
            prompt.push_str("\n- ");
            prompt.push_str(&task.title);
            let desc = task.description.trim();
            if !desc.is_empty() {
                prompt.push('：');
                prompt.push_str(desc);
            }
        }
        if !preferences.trim().is_empty() {
            prompt.push_str("\n组队偏好：");
            prompt.push_str(preferences.trim());
        }
        let plan_ids = tasks.iter().map(|t| t.id.clone()).collect();

        // Plan the team via LLM
        let draft = crate::services::draft_team_plan(
            Arc::clone(&self.ctx.db_path),
            Arc::clone(&self.ctx.secrets),
            None,
            &prompt,
            plan_ids,
            MAX_PLAN_MEMBERS,
        )
        .await
        .map_err(|e| HarnessError::PluginFailed {
            plugin: "agent_task".into(),
            phase: "plan",
            message: e.to_string(),
        })?;
        let plan_json = serde_json::to_string(&draft).map_err(|e| HarnessError::PluginFailed {
            plugin: "agent_task".into(),
            phase: "plan",
            message: format!("serialization error: {e}"),
        })?;

        // Record proposal in approvals inbox
        let db_path = Arc::clone(&self.ctx.db_path);
        let workspace_id = self.ctx.workspace_id.clone();
        let proposal_id = spawn_blocking(move || -> Result<String, HarnessError> {
            let db = Db::open(&db_path)?;
            crate::store::migrations::run(&db.0)?;
            let proposal = crate::domain::Proposal {
                id: new_id(),
                kind: "team_plan".into(),
                source: PROPOSAL_SOURCE.into(),
                plan_json,
                status: crate::domain::ProposalStatus::Pending,
                workspace_id,
                resolved_at: None,
                created_at: now_ms(),
            };
            let id = proposal.id.clone();
            repos::proposals::insert(&db.0, &proposal)?;
            Ok(id)
        })
        .await
        .map_err(|e| HarnessError::PluginFailed {
            plugin: "agent_task".into(),
            phase: "proposal",
            message: format!("join error: {e}"),
        })??;

        // Return SuspendRequest (the loop engine will deserialize this
        // and suspend the conversation).
        let suspend = SuspendRequest {
            confirm_handle: "resolve_proposal".into(),
            summary: format!(
                "已提交团队派发提案（{}个任务），等待用户在审批收件箱中确认。",
                tasks.len()
            ),
            entity_id: Some(proposal_id.clone()),
        };
        serde_json::to_string(&suspend).map_err(|e| HarnessError::PluginFailed {
            plugin: "agent_task".into(),
            phase: "suspend",
            message: format!("serialization error: {e}"),
        })
    }
}

/// Builds the two agent task dispatch tools for a conversation turn.
/// Called by the kernel when a RoleOverlay is present (ADR-0030).
pub fn build_agent_task_tools(ctx: AgentTaskToolContext) -> Vec<Arc<dyn Tool>> {
    let ctx = Arc::new(ctx);
    vec![
        Arc::new(CreateSelfTaskTool::new(Arc::clone(&ctx))),
        Arc::new(ProposeTeamDispatchTool::new(ctx)),
    ]
}
