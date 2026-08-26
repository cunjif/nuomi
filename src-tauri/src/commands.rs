//! All IPC commands, grouped by surface. Thin validation + forwarding only.
//!
//! Every command: `#[tauri::command] #[specta::specta]`, returns
//! `Result<T, IpcError>`, snake_case verb naming (ipc-contract rules).
//! The `#[tauri::command]`/invoke wiring is applied in `lib.rs`; the inner
//! `impl_*` free functions are testable without a Tauri runtime.

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use nuomi_core::domain::run_state::{ApprovalOutcome, RunEvent};
use nuomi_core::domain::{
    AgentProfile, CliFlavor, EventRecord, Integration, IntegrationKind, ProviderConfig,
    ProviderProtocol, Role, RunState, Schedule, Task, TaskStatus, Team, TeamTopology,
    WhiteBoardNote,
};
use nuomi_core::evolution::research::{
    online_authorized as core_online_authorized, set_online_authorized,
};
use nuomi_core::integrations::OutboundSink;
use nuomi_core::orchestrator::OrchestratorError;
use nuomi_core::plugins::approval_gate;
use nuomi_core::services::parse_schedule;
use nuomi_core::services::{run_team as core_run_team, TeamRunOutcome};
use nuomi_core::store::{migrations, repos, Db, StoreError};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{ChildStderr, ChildStdout};
use tokio_util::sync::CancellationToken;

use crate::ipc_error::IpcError;
use crate::state::AppState;

// ---------- sessions / chat ----------

pub async fn impl_create_session(state: &AppState) -> Result<SessionDto, IpcError> {
    let id = state.kernel.new_session().await?;
    let now = now_ms();
    Ok(SessionDto {
        id,
        title: "nuomi session".into(),
        created_at: now,
        updated_at: now,
    })
}

pub async fn impl_list_sessions(state: &AppState) -> Result<Vec<SessionDto>, IpcError> {
    Ok(state
        .kernel
        .list_sessions()
        .await?
        .into_iter()
        .map(|s| SessionDto {
            id: s.id,
            title: s.title,
            created_at: s.created_at,
            updated_at: s.updated_at,
        })
        .collect())
}

pub async fn impl_resume_session(state: &AppState, session_id: String) -> Result<(), IpcError> {
    state.kernel.resume(&session_id).await?;
    Ok(())
}

/// Cursor-paginated history for gap recovery (`afterSeq = 0` → full replay).
pub async fn impl_list_events(
    state: &AppState,
    session_id: String,
    after_seq: i64,
) -> Result<Vec<EventDto>, IpcError> {
    let path = state.db_path.clone();
    let records = tokio::task::spawn_blocking(move || -> Result<Vec<EventRecord>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        repos::sessions::get(&db.0, &session_id)?;
        Ok(repos::events::list_by_aggregate(
            &db.0,
            "session",
            &session_id,
            Some(after_seq),
        )?)
    })
    .await??;
    Ok(records.into_iter().map(EventDto::from).collect())
}

pub async fn impl_submit_task(
    state: &AppState,
    _session_id: String,
    input: String,
) -> Result<RunResultDto, IpcError> {
    let result = state.kernel.run_task(&input).await?;
    Ok(RunResultDto {
        final_text: result.final_text,
        steps: result.steps,
        truncated: result.truncated,
        session_id: state.kernel.session_id().await,
    })
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunResultDto {
    pub final_text: String,
    pub steps: usize,
    pub truncated: bool,
    pub session_id: String,
}

// ---------- tasks ----------

pub async fn impl_create_task(
    state: &AppState,
    title: String,
    description: String,
) -> Result<TaskDto, IpcError> {
    if title.trim().is_empty() {
        return Err(IpcError::new(
            "task.invalid_title",
            "task title must not be empty",
        ));
    }
    let task = Task {
        id: nuomi_core::domain::new_id(),
        session_id: Some(state.kernel.session_id().await),
        title,
        description,
        status: TaskStatus::Queued,
        created_at: now_ms(),
        updated_at: now_ms(),
    };
    let clone = task.clone();
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        repos::tasks_runs::insert_task(&db.0, &task)?;
        append_domain_event(
            &db.0,
            "task.created",
            task_id_payload(&task.id),
            task.created_at,
        )?;
        Ok(())
    })
    .await??;
    Ok(TaskDto::from(clone))
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TaskDto {
    pub id: String,
    pub session_id: Option<String>,
    pub title: String,
    pub description: String,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<Task> for TaskDto {
    fn from(t: Task) -> Self {
        Self {
            id: t.id,
            session_id: t.session_id,
            title: t.title,
            description: t.description,
            status: t.status.as_str().to_string(),
            created_at: t.created_at,
            updated_at: t.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EventDto {
    pub seq: i64,
    pub kind: String,
    pub payload: serde_json::Value,
    pub created_at: i64,
}

impl From<EventRecord> for EventDto {
    fn from(e: EventRecord) -> Self {
        Self {
            seq: e.seq,
            kind: e.kind,
            payload: e.payload,
            created_at: e.created_at,
        }
    }
}

pub async fn impl_list_tasks(
    state: &AppState,
    status: Option<String>,
) -> Result<Vec<TaskDto>, IpcError> {
    let parsed =
        match status.as_deref() {
            None => None,
            Some(s) => Some(TaskStatus::parse(s).ok_or_else(|| {
                IpcError::new("task.invalid_status", format!("unknown status {s}"))
            })?),
        };
    let path = state.db_path.clone();
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<Task>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(match parsed {
            Some(st) => repos::tasks_runs::list_tasks_by_status(&db.0, st, 500)?,
            None => repos::tasks_runs::list_tasks(&db.0, 500)?,
        })
    })
    .await??;
    Ok(rows.into_iter().map(TaskDto::from).collect())
}

/// Board transitions. `queued→running` auto-dispatches a run (SPEC D6):
/// creates the Run row, drives it to running, executes via the facade and
/// settles the terminal state — persisting each `state_changed` event first
/// (iron rule).
pub async fn impl_update_task_status(
    state: &AppState,
    task_id: String,
    status: String,
) -> Result<(), IpcError> {
    let next = TaskStatus::parse(&status)
        .ok_or_else(|| IpcError::new("task.invalid_status", format!("unknown status {status}")))?;
    let path = state.db_path.clone();
    let tid = task_id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let task = repos::tasks_runs::get_task(&db.0, &tid)?;
        let now = now_ms();
        repos::tasks_runs::update_task_status(&db.0, &tid, task.status, next, now)?;
        append_domain_event(
            &db.0,
            "task.status_changed",
            serde_json::json!({ "taskId": tid, "to": next.as_str() }),
            now,
        )?;
        Ok(())
    })
    .await??;

    if next == TaskStatus::Running {
        dispatch_run(state, task_id.clone()).await?;
    }
    if next == TaskStatus::Cancelled {
        // Board-level cancel stops any in-flight background team runs.
        state.run_cancels.cancel_by_task(&task_id);
    }
    Ok(())
}

async fn dispatch_run(state: &AppState, task_id: String) -> Result<String, IpcError> {
    let path = state.db_path.clone();
    let run_id = nuomi_core::domain::new_id();
    let session_id = state.kernel.session_id().await;
    let run = nuomi_core::domain::Run {
        id: run_id.clone(),
        task_id: task_id.clone(),
        session_id: session_id.clone(),
        status: RunState::Queued,
        heartbeat_at: now_ms(),
        created_at: now_ms(),
        updated_at: now_ms(),
    };
    {
        let path = path.clone();
        let run = run.clone();
        tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
            let db = Db::open(&path)?;
            migrations::run(&db.0)?;
            repos::tasks_runs::insert_run(&db.0, &run)?;
            transition_run(&db.0, &run.id, RunState::Queued, RunEvent::Start)?;
            Ok(())
        })
        .await??;
    }

    // Execute asynchronously; board updates arrive via `event://domain`.
    // The kernel is shared behind an Arc so the spawned task outlives the
    // command call.
    let kernel = state.kernel.clone();
    let rid = run_id.clone();
    tokio::spawn(async move {
        let result = kernel.run_task("execute dispatched task").await;
        let event = match &result {
            Ok(r) if !r.truncated => RunEvent::Succeed,
            _ => RunEvent::Fail,
        };
        let settled = tokio::task::spawn_blocking({
            let db_path = path.clone();
            let rid = rid.clone();
            move || -> Result<(), IpcError> {
                let db = Db::open(&db_path)?;
                migrations::run(&db.0)?;
                let current = repos::tasks_runs::get_run(&db.0, &rid)?.status;
                transition_run(&db.0, &rid, current, event)?;
                Ok(())
            }
        })
        .await;
        match settled {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!(error = %e, run = %rid, "failed to settle run state")
            }
            Err(e) => {
                tracing::warn!(error = %e, run = %rid, "settle task join failed")
            }
        }
    });

    Ok(run_id)
}

/// Deletes a board task with its children: approvals → runs → task in ONE
/// SQLite transaction (the 0002 FKs have no CASCADE, so order matters and
/// atomicity keeps observers from seeing half states). Running tasks are
/// refused — cancel first. The append-only `events` log is intentionally
/// left untouched: dangling aggregates are acceptable history.
pub async fn impl_delete_task(state: &AppState, task_id: String) -> Result<(), IpcError> {
    let path = state.db_path.clone();
    let tid = task_id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let task = match repos::tasks_runs::get_task(&db.0, &tid) {
            Ok(task) => task,
            Err(StoreError::NotFound { .. }) => {
                return Err(IpcError::new(
                    "task.not_found",
                    format!("task#{tid} not found"),
                ))
            }
            Err(e) => return Err(e.into()),
        };
        if matches!(task.status, TaskStatus::Running) {
            return Err(IpcError::new(
                "task.invalid_status",
                format!("task {} is running; cancel it before deleting", task.id),
            ));
        }
        if !repos::tasks_runs::delete_cascade(&db.0, &tid)? {
            return Err(IpcError::new(
                "task.not_found",
                format!("task#{tid} not found"),
            ));
        }
        append_domain_event(&db.0, "task.deleted", task_id_payload(&tid), now_ms())?;
        Ok(())
    })
    .await?
}

/// Applies one state-machine step with the persisted-event-first iron rule.
pub(crate) fn transition_run(
    conn: &rusqlite::Connection,
    run_id: &str,
    expected: RunState,
    ev: RunEvent,
) -> Result<RunState, IpcError> {
    transition_run_with_detail(conn, run_id, expected, ev, None)
}

/// [`transition_run`] optionally embedding an error message into the
/// `state_changed` payload (failed terminal states carry why they failed).
pub(crate) fn transition_run_with_detail(
    conn: &rusqlite::Connection,
    run_id: &str,
    expected: RunState,
    ev: RunEvent,
    error_message: Option<&str>,
) -> Result<RunState, IpcError> {
    let next = expected
        .transition(ev)
        .map_err(|e| IpcError::new("domain.invalid", e.to_string()))?;
    let now = now_ms();
    let mut payload = serde_json::json!({ "from": expected.as_str(), "to": next.as_str() });
    if let Some(message) = error_message {
        payload["error"] = serde_json::Value::String(message.to_string());
    }
    repos::events::append(conn, "run", run_id, "state_changed", &payload, now)?;
    repos::tasks_runs::update_run_status(conn, run_id, expected, next, now)?;
    Ok(next)
}

// ---------- runs ----------

pub async fn impl_get_run(state: &AppState, run_id: String) -> Result<RunDto, IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<RunDto, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(repos::tasks_runs::get_run(&db.0, &run_id)?.into())
    })
    .await?
}

pub async fn impl_list_runs_by_task(
    state: &AppState,
    task_id: String,
) -> Result<Vec<RunDto>, IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<Vec<RunDto>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(repos::tasks_runs::list_runs_by_task(&db.0, &task_id)?
            .into_iter()
            .map(Into::into)
            .collect())
    })
    .await?
}

// ---------- approvals ----------

pub async fn impl_list_pending_approvals(state: &AppState) -> Result<Vec<ApprovalDto>, IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<Vec<ApprovalDto>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(repos::tasks_runs::list_pending_approvals(&db.0)?
            .into_iter()
            .map(Into::into)
            .collect())
    })
    .await?
}

pub async fn impl_resolve_approval(
    state: &AppState,
    approval_id: String,
    approved: bool,
) -> Result<(), IpcError> {
    approval_gate::resolve_approval(
        state.db_path.clone(),
        &approval_id,
        if approved {
            ApprovalOutcome::Approved
        } else {
            ApprovalOutcome::Denied
        },
    )
    .await?;
    Ok(())
}

// ---------- workspace ----------

pub async fn impl_list_dir(state: &AppState, path: String) -> Result<Vec<FileEntryDto>, IpcError> {
    let ws = state.workspace()?;
    Ok(ws
        .list_dir(&path)?
        .into_iter()
        .map(|e| FileEntryDto {
            name: e.name,
            is_dir: e.is_dir,
            size: e.size,
        })
        .collect())
}

pub async fn impl_read_file(state: &AppState, path: String) -> Result<String, IpcError> {
    Ok(state.workspace()?.read_file(&path)?)
}

pub async fn impl_write_file(
    state: &AppState,
    path: String,
    content: String,
) -> Result<(), IpcError> {
    state.workspace()?.write_file_atomic(&path, &content)?;
    Ok(())
}

// ---------- git ----------

pub async fn impl_git_status(state: &AppState) -> Result<Vec<GitStatusDto>, IpcError> {
    Ok(state
        .git()
        .status()
        .await?
        .into_iter()
        .map(|e| GitStatusDto {
            index_status: e.index_status.to_string(),
            worktree_status: e.worktree_status.to_string(),
            path: e.path,
        })
        .collect())
}

pub async fn impl_git_log(state: &AppState, limit: u32) -> Result<Vec<GitCommitDto>, IpcError> {
    Ok(state
        .git()
        .log(limit)
        .await?
        .into_iter()
        .map(|c| GitCommitDto {
            hash: c.hash,
            subject: c.subject,
            author: c.author,
        })
        .collect())
}

pub async fn impl_git_stage(state: &AppState, paths: Vec<String>) -> Result<(), IpcError> {
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    state.git().stage(&refs).await?;
    Ok(())
}

pub async fn impl_git_commit(state: &AppState, message: String) -> Result<String, IpcError> {
    if message.trim().is_empty() {
        return Err(IpcError::new(
            "git.empty_message",
            "commit message must not be empty",
        ));
    }
    Ok(state.git().commit(&message).await?)
}

pub async fn impl_git_push(
    state: &AppState,
    remote: String,
    branch: String,
) -> Result<String, IpcError> {
    Ok(state.git().push(&remote, &branch).await?)
}

pub async fn impl_git_worktrees(state: &AppState) -> Result<Vec<GitWorktreeDto>, IpcError> {
    Ok(state
        .git()
        .list_worktrees()
        .await?
        .into_iter()
        .map(|w| GitWorktreeDto {
            is_current: w.path == state.current_workspace().to_string_lossy(),
            path: w.path,
            head: w.head,
            branch: w.branch,
        })
        .collect())
}

// ---------- schedules ----------

pub async fn impl_create_schedule(
    state: &AppState,
    name: String,
    cron_expr: String,
    task_title: String,
    task_description: String,
) -> Result<ScheduleDto, IpcError> {
    // Validate expression up-front (parse errors surface immediately).
    parse_schedule(&cron_expr)
        .map_err(|e| IpcError::new("scheduler.bad_expression", e.to_string()))?;
    let schedule = Schedule {
        id: nuomi_core::domain::new_id(),
        name,
        cron_expr,
        task_title,
        task_description,
        enabled: true,
        last_triggered_at: None,
        next_trigger_at: None,
        created_at: now_ms(),
        updated_at: now_ms(),
    };
    let clone = schedule.clone();
    let path = state.db_path.clone();
    let created = tokio::task::spawn_blocking(move || -> Result<Schedule, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        repos::tasks_runs::insert_schedule(&db.0, &schedule)?;
        append_domain_event(
            &db.0,
            "schedule.created",
            serde_json::json!({ "scheduleId": clone.id }),
            clone.created_at,
        )?;
        Ok(clone)
    })
    .await??;
    Ok(ScheduleDto::from(created))
}

pub async fn impl_list_schedules(state: &AppState) -> Result<Vec<ScheduleDto>, IpcError> {
    let path = state.db_path.clone();
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<Schedule>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(repos::tasks_runs::list_schedules(&db.0, 200)?)
    })
    .await??;
    Ok(rows.into_iter().map(ScheduleDto::from).collect())
}

pub async fn impl_toggle_schedule(
    state: &AppState,
    schedule_id: String,
    enabled: bool,
) -> Result<(), IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        repos::tasks_runs::set_schedule_enabled(&db.0, &schedule_id, enabled, now_ms())?;
        Ok(())
    })
    .await?
}

pub async fn impl_delete_schedule(state: &AppState, schedule_id: String) -> Result<(), IpcError> {
    let path = state.db_path.clone();
    let sid = schedule_id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let mut s = match repos::tasks_runs::get_schedule(&db.0, &sid) {
            Ok(s) => s,
            Err(e) => return Err(e.into()),
        };
        s.enabled = false;
        s.updated_at = now_ms();
        // v1: soft-delete by disable+rename tombstone (append-only philosophy
        // lives in events; schedules table has no delete guard, but we keep
        // history via the domain event below).
        s.name = format!("{}#deleted#{}", s.name, s.id);
        repos::tasks_runs::update_schedule(&db.0, &s)?;
        append_domain_event(
            &db.0,
            "schedule.deleted",
            serde_json::json!({ "scheduleId": sid }),
            now_ms(),
        )?;
        Ok(())
    })
    .await?
}

// ---------- settings / providers ----------

#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub name: String,
    pub protocol: ProviderProtocolDto,
    pub base_url: String,
    pub capabilities: Vec<String>,
    pub is_master: bool,
    /// Plaintext only in transit — stored straight into the OS keyring,
    /// never persisted to SQLite or logs.
    pub api_key: Option<String>,
}

pub async fn impl_upsert_provider(
    state: &AppState,
    provider: ProviderInput,
) -> Result<(), IpcError> {
    use nuomi_core::providers::SecretStore;
    let keyring_ref = provider
        .api_key
        .as_ref()
        .map(|_| format!("provider/{}", provider.name));
    if let (Some(key), Some(reference)) = (provider.api_key.as_ref(), keyring_ref.as_ref()) {
        let store = nuomi_core::providers::OsKeyring;
        store.set(reference, key).await?;
    }
    let config = ProviderConfig {
        id: nuomi_core::domain::new_id(),
        name: provider.name,
        protocol: match provider.protocol {
            ProviderProtocolDto::OpenAiCompatible => ProviderProtocol::OpenAiCompatible,
            ProviderProtocolDto::AnthropicCompatible => ProviderProtocol::AnthropicCompatible,
        },
        base_url: provider.base_url,
        keyring_ref,
        capabilities: provider.capabilities,
        is_master: provider.is_master,
        fallback_order: None,
        params: serde_json::json!({}),
        created_at: now_ms(),
        updated_at: now_ms(),
    };
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(repos::providers::insert_provider(&db.0, &config)?)
    })
    .await?
}

pub async fn impl_list_providers(state: &AppState) -> Result<Vec<ProviderDto>, IpcError> {
    let path = state.db_path.clone();
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<ProviderConfig>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(repos::providers::list_providers(&db.0)?)
    })
    .await??;
    Ok(rows
        .into_iter()
        .map(|p| ProviderDto {
            id: p.id,
            name: p.name,
            protocol: match p.protocol {
                ProviderProtocol::OpenAiCompatible => ProviderProtocolDto::OpenAiCompatible,
                ProviderProtocol::AnthropicCompatible => ProviderProtocolDto::AnthropicCompatible,
            },
            base_url: p.base_url,
            has_key: p.keyring_ref.is_some(),
            capabilities: p.capabilities,
            is_master: p.is_master,
        })
        .collect())
}

pub async fn impl_set_sensitive_tools(
    state: &AppState,
    patterns: Vec<String>,
) -> Result<(), IpcError> {
    let mem = nuomi_core::plugins::MemoryService::new(state.db_path.clone());
    approval_gate::set_sensitive_tools(&mem, &patterns).await?;
    Ok(())
}

pub async fn impl_get_sensitive_tools(state: &AppState) -> Result<Option<Vec<String>>, IpcError> {
    let mem = nuomi_core::plugins::MemoryService::new(state.db_path.clone());
    Ok(approval_gate::read_sensitive_tools(&mem)
        .await?
        .map(|p| p.patterns))
}

pub async fn impl_set_online_authorized(
    state: &AppState,
    authorized: bool,
) -> Result<(), IpcError> {
    let mem = nuomi_core::plugins::MemoryService::new(state.db_path.clone());
    set_online_authorized(&mem, authorized).await?;
    Ok(())
}

pub async fn impl_get_online_authorized(state: &AppState) -> Result<bool, IpcError> {
    let mem = nuomi_core::plugins::MemoryService::new(state.db_path.clone());
    Ok(core_online_authorized(&mem).await)
}

// ---------- cli agents (SPEC cli-agents-m1 C4) ----------

/// Probe budget for `check_cli_agent` (`--version` run).
const CLI_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Upper bound for stderr excerpts embedded in check results.
const CHECK_STDERR_MAX_CHARS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CliFlavorDto {
    ClaudeCode,
    Codex,
    Plain,
}

fn flavor_to_dto(f: CliFlavor) -> CliFlavorDto {
    match f {
        CliFlavor::ClaudeCode => CliFlavorDto::ClaudeCode,
        CliFlavor::Codex => CliFlavorDto::Codex,
        CliFlavor::Plain => CliFlavorDto::Plain,
    }
}

fn flavor_from_dto(f: CliFlavorDto) -> CliFlavor {
    match f {
        CliFlavorDto::ClaudeCode => CliFlavor::ClaudeCode,
        CliFlavorDto::Codex => CliFlavor::Codex,
        CliFlavorDto::Plain => CliFlavor::Plain,
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileDto {
    pub id: String,
    pub name: String,
    pub adapter: String,
    pub flavor: CliFlavorDto,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub working_dir: Option<String>,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

impl TryFrom<AgentProfile> for AgentProfileDto {
    type Error = IpcError;

    fn try_from(p: AgentProfile) -> Result<Self, Self::Error> {
        Ok(Self {
            id: p.id,
            name: p.name,
            adapter: p.adapter,
            flavor: flavor_to_dto(p.flavor),
            command: p.command,
            args: decode_string_array(p.args)?,
            env: decode_string_map(p.env)?,
            working_dir: p.working_dir,
            enabled: p.enabled,
            created_at: p.created_at,
            updated_at: p.updated_at,
        })
    }
}

fn decode_string_array(value: serde_json::Value) -> Result<Vec<String>, IpcError> {
    serde_json::from_value(value).map_err(|e| {
        IpcError::new(
            "agent_profile.invalid",
            format!("args must be a JSON array of strings: {e}"),
        )
    })
}

fn decode_string_map(value: serde_json::Value) -> Result<BTreeMap<String, String>, IpcError> {
    serde_json::from_value(value).map_err(|e| {
        IpcError::new(
            "agent_profile.invalid",
            format!("env must be a JSON object of strings: {e}"),
        )
    })
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentProfileInput {
    pub name: String,
    pub flavor: CliFlavorDto,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub working_dir: Option<String>,
    pub enabled: bool,
}

impl AgentProfileInput {
    fn validate(&self) -> Result<(), IpcError> {
        if self.name.trim().is_empty() {
            return Err(IpcError::new(
                "agent_profile.invalid",
                "agent profile name must not be empty",
            ));
        }
        if self.command.trim().is_empty() {
            return Err(IpcError::new(
                "agent_profile.invalid",
                "agent profile command must not be empty",
            ));
        }
        Ok(())
    }

    fn into_entity(self, id: String, created_at: i64, updated_at: i64) -> AgentProfile {
        AgentProfile {
            id,
            name: self.name,
            adapter: "cli".into(),
            flavor: flavor_from_dto(self.flavor),
            command: self.command,
            args: serde_json::json!(self.args),
            env: serde_json::json!(self.env),
            working_dir: self.working_dir,
            enabled: self.enabled,
            created_at,
            updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CliAgentCheckDto {
    pub ok: bool,
    pub version_line: Option<String>,
    pub error: Option<String>,
}

fn failed_check(message: String) -> CliAgentCheckDto {
    CliAgentCheckDto {
        ok: false,
        version_line: None,
        error: Some(message),
    }
}

fn truncate_chars(raw: &str, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        raw.to_string()
    } else {
        raw.chars().take(max_chars).collect()
    }
}

/// Drains probe pipes so a chatty `--version` cannot deadlock `wait()`;
/// returns the first non-empty stdout line plus the truncated stderr.
async fn collect_probe_output(
    stdout: Option<ChildStdout>,
    stderr: Option<ChildStderr>,
) -> (Option<String>, String) {
    let mut first_line = None;
    if let Some(out) = stdout {
        let mut reader = BufReader::new(out);
        let mut buf = String::new();
        loop {
            buf.clear();
            match reader.read_line(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let trimmed = buf.trim();
                    if !trimmed.is_empty() {
                        first_line = Some(trimmed.to_string());
                        break;
                    }
                }
            }
        }
    }
    let mut err_text = String::new();
    if let Some(mut err) = stderr {
        // Errors while draining are irrelevant — the exit status decides.
        let _ = err.read_to_string(&mut err_text).await;
    }
    (
        first_line,
        truncate_chars(&err_text, CHECK_STDERR_MAX_CHARS),
    )
}

pub async fn impl_list_agent_profiles(state: &AppState) -> Result<Vec<AgentProfileDto>, IpcError> {
    let path = state.db_path.clone();
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<AgentProfile>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(repos::agent_profiles::list(&db.0)?)
    })
    .await??;
    rows.into_iter().map(AgentProfileDto::try_from).collect()
}

/// `name` is the idempotency key: an existing profile with the same name is
/// updated in place (keeping its `id`/`created_at`), otherwise inserted fresh.
pub async fn impl_upsert_agent_profile(
    state: &AppState,
    profile: AgentProfileInput,
) -> Result<AgentProfileDto, IpcError> {
    profile.validate()?;
    let path = state.db_path.clone();
    let entity = tokio::task::spawn_blocking(move || -> Result<AgentProfile, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let now = now_ms();
        let existing = repos::agent_profiles::list(&db.0)?
            .into_iter()
            .find(|p| p.name == profile.name);
        match existing {
            Some(mut prev) => {
                prev.flavor = flavor_from_dto(profile.flavor);
                prev.command = profile.command;
                prev.args = serde_json::json!(profile.args);
                prev.env = serde_json::json!(profile.env);
                prev.working_dir = profile.working_dir;
                prev.enabled = profile.enabled;
                prev.updated_at = now;
                repos::agent_profiles::update(&db.0, &prev)?;
                Ok(prev)
            }
            None => {
                let fresh = profile.into_entity(nuomi_core::domain::new_id(), now, now);
                repos::agent_profiles::insert(&db.0, &fresh)?;
                Ok(fresh)
            }
        }
    })
    .await??;
    AgentProfileDto::try_from(entity)
}

pub async fn impl_delete_agent_profile(
    state: &AppState,
    profile_id: String,
) -> Result<(), IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        if !repos::agent_profiles::delete(&db.0, &profile_id)? {
            return Err(IpcError::new(
                "agent_profile.not_found",
                format!("agent_profile#{profile_id} not found"),
            ));
        }
        Ok(())
    })
    .await?
}

/// Availability probe: runs the stored command with `--version` (argv array,
/// no allowlist — the check exists precisely to confirm executability).
/// The child is `kill_on_drop(true)` with stdin nulled and the whole run is
/// capped at [`CLI_PROBE_TIMEOUT`]; failures surface as `ok:false` results,
/// never as IPC errors (except unknown profile ids).
pub async fn impl_check_cli_agent(
    state: &AppState,
    profile_id: String,
) -> Result<CliAgentCheckDto, IpcError> {
    let path = state.db_path.clone();
    let profile = tokio::task::spawn_blocking(move || -> Result<AgentProfile, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        match repos::agent_profiles::get(&db.0, &profile_id) {
            Ok(p) => Ok(p),
            Err(StoreError::NotFound { .. }) => Err(IpcError::new(
                "agent_profile.not_found",
                format!("agent_profile#{profile_id} not found"),
            )),
            Err(e) => Err(e.into()),
        }
    })
    .await??;

    let mut cmd = tokio::process::Command::new(&profile.command);
    cmd.arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            return Ok(failed_check(format!(
                "failed to spawn {}: {e}",
                profile.command
            )))
        }
    };
    // Drain pipes concurrently so a chatty process cannot deadlock `wait()`.
    let reader = tokio::spawn(collect_probe_output(
        child.stdout.take(),
        child.stderr.take(),
    ));

    match tokio::time::timeout(CLI_PROBE_TIMEOUT, child.wait()).await {
        Err(_elapsed) => {
            let _ = child.kill().await;
            reader.abort();
            Ok(failed_check(format!(
                "timeout after {}s",
                CLI_PROBE_TIMEOUT.as_secs()
            )))
        }
        Ok(Err(e)) => {
            reader.abort();
            Ok(failed_check(format!("wait failed: {e}")))
        }
        Ok(Ok(status)) => {
            let (version_line, stderr_text) = reader.await.unwrap_or((None, String::new()));
            if !status.success() {
                return Ok(failed_check(format!(
                    "{} exited with {status}; stderr: {stderr_text}",
                    profile.command
                )));
            }
            match version_line {
                Some(line) => Ok(CliAgentCheckDto {
                    ok: true,
                    version_line: Some(line),
                    error: None,
                }),
                None => Ok(failed_check(format!(
                    "no version output on stdout; stderr: {stderr_text}"
                ))),
            }
        }
    }
}

// ---------- roles / teams / whiteboard (SPEC team-shell-m1 T4) ----------

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RoleDto {
    pub id: String,
    pub name: String,
    pub provider_id: Option<String>,
    pub system_prompt_override: Option<String>,
    pub tool_allowlist: Vec<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i64>,
    pub params: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<Role> for RoleDto {
    fn from(r: Role) -> Self {
        Self {
            id: r.id,
            name: r.name,
            provider_id: r.provider_id,
            system_prompt_override: r.system_prompt_override,
            tool_allowlist: r.tool_allowlist,
            temperature: r.temperature,
            max_tokens: r.max_tokens,
            params: r.params,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RoleInput {
    pub name: String,
    pub provider_id: Option<String>,
    pub system_prompt_override: Option<String>,
    pub tool_allowlist: Vec<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i64>,
    /// Free-form extras; the `agent_profile_id` key binds a CLI agent
    /// profile (SPEC team-shell-m1 D2b).
    pub params: serde_json::Value,
}

impl RoleInput {
    fn validate(&self) -> Result<(), IpcError> {
        if self.name.trim().is_empty() {
            return Err(IpcError::new("role.invalid", "role name must not be empty"));
        }
        Ok(())
    }
}

pub async fn impl_list_roles(state: &AppState) -> Result<Vec<RoleDto>, IpcError> {
    let path = state.db_path.clone();
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<Role>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(repos::roles::list(&db.0)?)
    })
    .await??;
    Ok(rows.into_iter().map(RoleDto::from).collect())
}

/// `name` is the idempotency key: an existing role with the same name is
/// updated in place (keeping its `id`/`created_at`), otherwise inserted.
pub async fn impl_upsert_role(state: &AppState, role: RoleInput) -> Result<RoleDto, IpcError> {
    role.validate()?;
    let path = state.db_path.clone();
    let entity = tokio::task::spawn_blocking(move || -> Result<Role, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let now = now_ms();
        let existing = repos::roles::list(&db.0)?
            .into_iter()
            .find(|r| r.name == role.name);
        match existing {
            Some(mut prev) => {
                prev.provider_id = role.provider_id;
                prev.system_prompt_override = role.system_prompt_override;
                prev.tool_allowlist = role.tool_allowlist;
                prev.temperature = role.temperature;
                prev.max_tokens = role.max_tokens;
                prev.params = role.params;
                prev.updated_at = now;
                repos::roles::update(&db.0, &prev)?;
                Ok(prev)
            }
            None => {
                let fresh = Role {
                    id: nuomi_core::domain::new_id(),
                    name: role.name,
                    provider_id: role.provider_id,
                    system_prompt_override: role.system_prompt_override,
                    tool_allowlist: role.tool_allowlist,
                    temperature: role.temperature,
                    max_tokens: role.max_tokens,
                    params: role.params,
                    created_at: now,
                    updated_at: now,
                };
                repos::roles::insert(&db.0, &fresh)?;
                Ok(fresh)
            }
        }
    })
    .await??;
    Ok(RoleDto::from(entity))
}

pub async fn impl_delete_role(state: &AppState, role_id: String) -> Result<(), IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        if !repos::roles::delete(&db.0, &role_id)? {
            return Err(IpcError::new(
                "role.not_found",
                format!("role#{role_id} not found"),
            ));
        }
        Ok(())
    })
    .await?
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TeamTopologyDto {
    Pipeline,
    Router,
    GroupChat,
}

fn topology_to_dto(t: TeamTopology) -> TeamTopologyDto {
    match t {
        TeamTopology::Pipeline => TeamTopologyDto::Pipeline,
        TeamTopology::Router => TeamTopologyDto::Router,
        TeamTopology::GroupChat => TeamTopologyDto::GroupChat,
    }
}

fn topology_from_dto(t: TeamTopologyDto) -> TeamTopology {
    match t {
        TeamTopologyDto::Pipeline => TeamTopology::Pipeline,
        TeamTopologyDto::Router => TeamTopology::Router,
        TeamTopologyDto::GroupChat => TeamTopology::GroupChat,
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TeamDto {
    pub id: String,
    pub name: String,
    pub topology: TeamTopologyDto,
    pub member_role_ids: Vec<String>,
    pub config: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<Team> for TeamDto {
    fn from(t: Team) -> Self {
        Self {
            id: t.id,
            name: t.name,
            topology: topology_to_dto(t.topology),
            member_role_ids: t.member_role_ids,
            config: t.config,
            created_at: t.created_at,
            updated_at: t.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TeamInput {
    pub name: String,
    pub topology: TeamTopologyDto,
    pub member_role_ids: Vec<String>,
    /// Topology-specific config (`max_rounds`, selector settings, ...).
    pub config: serde_json::Value,
}

impl TeamInput {
    fn validate(&self) -> Result<(), IpcError> {
        if self.name.trim().is_empty() {
            return Err(IpcError::new("team.invalid", "team name must not be empty"));
        }
        if self.member_role_ids.is_empty() {
            return Err(IpcError::new(
                "team.member_missing",
                "team needs at least one member role",
            ));
        }
        Ok(())
    }

    fn into_entity(self, id: String, created_at: i64, updated_at: i64) -> Team {
        Team {
            id,
            name: self.name,
            topology: topology_from_dto(self.topology),
            member_role_ids: self.member_role_ids,
            config: self.config,
            created_at,
            updated_at,
        }
    }
}

pub async fn impl_list_teams(state: &AppState) -> Result<Vec<TeamDto>, IpcError> {
    let path = state.db_path.clone();
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<Team>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(repos::teams::list(&db.0)?)
    })
    .await??;
    Ok(rows.into_iter().map(TeamDto::from).collect())
}

/// `name` is the idempotency key; every `memberRoleIds` entry must exist in
/// `roles` or the whole upsert fails with `"team.member_missing"`.
pub async fn impl_upsert_team(state: &AppState, team: TeamInput) -> Result<TeamDto, IpcError> {
    team.validate()?;
    let path = state.db_path.clone();
    let entity = tokio::task::spawn_blocking(move || -> Result<Team, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let missing: Vec<String> = team
            .member_role_ids
            .iter()
            .filter(|rid| repos::roles::get(&db.0, rid).is_err())
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(IpcError::with_details(
                "team.member_missing",
                format!("unknown member roles: {}", missing.join(", ")),
                serde_json::json!({ "missing": missing }),
            ));
        }
        let now = now_ms();
        let existing = repos::teams::list(&db.0)?
            .into_iter()
            .find(|t| t.name == team.name);
        match existing {
            Some(mut prev) => {
                prev.topology = topology_from_dto(team.topology);
                prev.member_role_ids = team.member_role_ids;
                prev.config = team.config;
                prev.updated_at = now;
                repos::teams::update(&db.0, &prev)?;
                Ok(prev)
            }
            None => {
                let fresh = team.into_entity(nuomi_core::domain::new_id(), now, now);
                repos::teams::insert(&db.0, &fresh)?;
                Ok(fresh)
            }
        }
    })
    .await??;
    Ok(TeamDto::from(entity))
}

pub async fn impl_delete_team(state: &AppState, team_id: String) -> Result<(), IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        if !repos::teams::delete(&db.0, &team_id)? {
            return Err(IpcError::new(
                "team.not_found",
                format!("team#{team_id} not found"),
            ));
        }
        Ok(())
    })
    .await?
}

// ---------- team formation (SPEC auto-team-m1 F3) ----------

/// Member ceiling handed to the planner (SPEC auto-team-m1 §5: parameterized,
/// default 5; the UI entry point uses the default).
const FORM_TEAM_MAX_MEMBERS: usize = 5;

/// Maps [`services::team_former`] failures onto stable IPC codes.
///
/// Deliberately separate from [`map_orchestrator_error`]: that one must keep
/// mapping every `InvalidTeam` of `run_team` onto `"team.invalid_config"`,
/// while formation refines the same error type into planner-specific codes
/// (SPEC auto-team-m1 D6/§5).
fn map_form_error(e: OrchestratorError) -> IpcError {
    match e {
        OrchestratorError::InvalidTeam(msg) => {
            if msg.contains("no planner") {
                IpcError::new("team.no_planner", msg)
            } else if msg.contains("plan invalid")
                || msg.contains("unknown members")
                || msg.contains("member(s)")
            {
                IpcError::with_details(
                    "team.plan_invalid",
                    msg.clone(),
                    serde_json::json!({ "reason": msg }),
                )
            } else {
                IpcError::new("team.invalid_config", msg)
            }
        }
        other => IpcError::new("team.form_failed", other.to_string()),
    }
}

/// LLM-planned team formation for a raw task text: asks the default provider
/// (master first) to compose a plan from the live catalog, validates it
/// before any write, then persists new member roles + the team row and
/// returns the persisted [`TeamDto`]; its `id` feeds `run_team_on_task`
/// directly (two-step UI flow, SPEC auto-team-m1 D7).
pub async fn impl_form_team(
    state: &AppState,
    task: String,
    session_id: Option<String>,
) -> Result<TeamDto, IpcError> {
    if task.trim().is_empty() {
        return Err(IpcError::new("task.invalid", "task must not be empty"));
    }
    let formed = nuomi_core::services::form_team(
        state.db_path.clone(),
        Some(state.kernel.context().bus()),
        state.secrets.clone(),
        Some(state.current_workspace()),
        session_id.as_deref(),
        &task,
        FORM_TEAM_MAX_MEMBERS,
    )
    .await
    .map_err(map_form_error)?;

    // Read back through the repository so the DTO mirrors the persisted row.
    let path = state.db_path.clone();
    let team_id = formed.team.id;
    let team = tokio::task::spawn_blocking(move || -> Result<Team, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(repos::teams::get(&db.0, &team_id)?)
    })
    .await??;
    Ok(TeamDto::from(team))
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TeamPlanMemberDto {
    pub kind: String,
    pub ref_id: String,
    pub name: String,
    pub will_create_role: bool,
}

/// Dry-run projection of a validated formation plan: what a commit would
/// build, without touching roles/teams/events (打磨③a).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TeamPlanDto {
    pub topology: TeamTopologyDto,
    pub members: Vec<TeamPlanMemberDto>,
    pub max_rounds: Option<u32>,
    pub required: Vec<String>,
    pub rationale: String,
}

impl From<nuomi_core::services::TeamPlan> for TeamPlanDto {
    fn from(plan: nuomi_core::services::TeamPlan) -> Self {
        Self {
            topology: topology_to_dto(plan.topology),
            members: plan
                .members
                .into_iter()
                .map(|m| TeamPlanMemberDto {
                    kind: m.kind,
                    ref_id: m.ref_id,
                    name: m.name,
                    will_create_role: m.will_create_role,
                })
                .collect(),
            max_rounds: plan.max_rounds,
            required: plan.required,
            rationale: plan.rationale,
        }
    }
}

/// Zero-write formation preview: identical task validation and error codes as
/// [`impl_form_team`] (`task.invalid`, then the `map_form_error` family), but
/// no bus is attached and nothing is persisted.
pub async fn impl_preview_team(state: &AppState, task: String) -> Result<TeamPlanDto, IpcError> {
    if task.trim().is_empty() {
        return Err(IpcError::new("task.invalid", "task must not be empty"));
    }
    let plan = nuomi_core::services::preview_team(
        state.db_path.clone(),
        state.secrets.clone(),
        Some(state.current_workspace()),
        &task,
        FORM_TEAM_MAX_MEMBERS,
    )
    .await
    .map_err(map_form_error)?;
    Ok(TeamPlanDto::from(plan))
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WhiteBoardNoteDto {
    pub id: String,
    pub session_id: String,
    pub author_role_id: Option<String>,
    pub note_type: String,
    pub body: String,
    pub refs: serde_json::Value,
    pub seq: i64,
    pub created_at: i64,
}

impl From<WhiteBoardNote> for WhiteBoardNoteDto {
    fn from(n: WhiteBoardNote) -> Self {
        Self {
            id: n.id,
            session_id: n.session_id,
            author_role_id: n.author_role_id,
            note_type: n.note_type,
            body: n.body,
            refs: n.refs,
            seq: n.seq,
            created_at: n.created_at,
        }
    }
}

pub async fn impl_list_whiteboard_notes(
    state: &AppState,
    session_id: String,
) -> Result<Vec<WhiteBoardNoteDto>, IpcError> {
    let path = state.db_path.clone();
    let notes = tokio::task::spawn_blocking(move || -> Result<Vec<WhiteBoardNote>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(repos::whiteboard::list_by_session(&db.0, &session_id)?)
    })
    .await??;
    Ok(notes.into_iter().map(WhiteBoardNoteDto::from).collect())
}

/// Starts a supervised background team run for a board task:
/// validates task + team, creates the Run row and drives queued→running
/// (persisting each `state_changed` first — iron rule), then spawns the
/// executor. Terminal state settles asynchronously via `event://domain`.
pub async fn impl_run_team_on_task(
    state: &AppState,
    task_id: String,
    team_id: String,
) -> Result<RunDto, IpcError> {
    #[derive(Debug)]
    struct PreparedTeamRun {
        run: nuomi_core::domain::Run,
        task_text: String,
    }

    let fallback_session = state.kernel.session_id().await;
    let path = state.db_path.clone();
    let tid = task_id;
    let team = team_id.clone();
    let prepared = tokio::task::spawn_blocking(move || -> Result<PreparedTeamRun, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let task = match repos::tasks_runs::get_task(&db.0, &tid) {
            Ok(task) => task,
            Err(StoreError::NotFound { .. }) => {
                return Err(IpcError::new(
                    "task.not_found",
                    format!("task#{tid} not found"),
                ))
            }
            Err(e) => return Err(e.into()),
        };
        if !matches!(task.status, TaskStatus::Backlog | TaskStatus::Queued) {
            return Err(IpcError::new(
                "task.invalid_status",
                format!(
                    "task {} is '{}'; only backlog/queued tasks can start a team run",
                    task.id,
                    task.status.as_str()
                ),
            ));
        }
        if let Err(StoreError::NotFound { .. }) = repos::teams::get(&db.0, &team) {
            return Err(IpcError::new(
                "team.not_found",
                format!("team#{team} not found"),
            ));
        }
        let task_text = if task.description.is_empty() {
            task.title.clone()
        } else {
            format!("{}\n{}", task.title, task.description)
        };
        let now = now_ms();
        let run = nuomi_core::domain::Run {
            id: nuomi_core::domain::new_id(),
            task_id: tid,
            session_id: task.session_id.unwrap_or(fallback_session),
            status: RunState::Queued,
            heartbeat_at: now,
            created_at: now,
            updated_at: now,
        };
        repos::tasks_runs::insert_run(&db.0, &run)?;
        // Iron rule: persist queued→running BEFORE spawning any executor.
        transition_run(&db.0, &run.id, RunState::Queued, RunEvent::Start)?;
        Ok(PreparedTeamRun { run, task_text })
    })
    .await??;

    spawn_team_run(
        state,
        prepared.run.id.clone(),
        prepared.run.task_id.clone(),
        prepared.run.session_id.clone(),
        team_id,
        prepared.task_text,
    );
    Ok(prepared.run.into())
}

enum TeamSettlement {
    Succeeded(TeamRunOutcome),
    Failed(String),
    Cancelled,
}

/// Supervised background execution of one team run: races the executor
/// against the run's cancellation token and settles the terminal state with
/// the persisted-event-first iron rule. Panics/join failures inside the
/// executor degrade to `failed` instead of leaving the run stuck running.
fn spawn_team_run(
    state: &AppState,
    run_id: String,
    task_id: String,
    session_id: String,
    team_id: String,
    task_text: String,
) {
    let token = CancellationToken::new();
    state.run_cancels.register(&run_id, &task_id, token.clone());

    let db_path = state.db_path.clone();
    let secrets = state.secrets.clone();
    let cwd = Some(state.current_workspace());
    let bus = state.kernel.context().bus();
    let registry = state.run_cancels.clone();

    tokio::spawn(async move {
        let mut exec = tokio::spawn({
            let db_path = db_path.clone();
            async move {
                core_run_team(
                    db_path,
                    Some(bus),
                    &team_id,
                    &session_id,
                    &task_text,
                    secrets,
                    cwd,
                )
                .await
            }
        });

        // Finished executors win over a simultaneous cancellation.
        let settlement: TeamSettlement = tokio::select! {
            biased;
            joined = &mut exec => match joined {
                Ok(Ok(outcome)) => TeamSettlement::Succeeded(outcome),
                Ok(Err(e)) => TeamSettlement::Failed(e.to_string()),
                Err(join_err) => {
                    TeamSettlement::Failed(format!("team run executor join failed: {join_err}"))
                }
            },
            _ = token.cancelled() => {
                exec.abort();
                TeamSettlement::Cancelled
            }
        };

        let rid = run_id.clone();
        if let TeamSettlement::Succeeded(outcome) = &settlement {
            tracing::info!(
                run = %run_id,
                rounds = outcome.rounds,
                converged = outcome.converged,
                "team run finished"
            );
        }
        let settled = tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
            let db = Db::open(&db_path)?;
            migrations::run(&db.0)?;
            let current = repos::tasks_runs::get_run(&db.0, &rid)?.status;
            match settlement {
                TeamSettlement::Succeeded(_) => {
                    transition_run(&db.0, &rid, current, RunEvent::Succeed).map(|_| ())
                }
                TeamSettlement::Failed(message) => {
                    transition_run_with_detail(&db.0, &rid, current, RunEvent::Fail, Some(&message))
                        .map(|_| ())
                }
                TeamSettlement::Cancelled => {
                    transition_run(&db.0, &rid, current, RunEvent::Cancel).map(|_| ())
                }
            }
        })
        .await;
        match settled {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!(error = %e, run = %run_id, "failed to settle team run"),
            Err(e) => tracing::warn!(error = %e, run = %run_id, "team run settle join failed"),
        }
        registry.remove(&run_id);
    });
}

/// Session-level team execution entry (REPL / no-task scenario): runs
/// synchronously and returns the outcome directly; no `tasks_runs` row is
/// created (SPEC D5).
pub async fn impl_run_team_session(
    state: &AppState,
    session_id: String,
    team_id: String,
    task: String,
) -> Result<TeamRunResultDto, IpcError> {
    let outcome = core_run_team(
        state.db_path.clone(),
        Some(state.kernel.context().bus()),
        &team_id,
        &session_id,
        &task,
        state.secrets.clone(),
        Some(state.current_workspace()),
    )
    .await
    .map_err(map_orchestrator_error)?;
    Ok(TeamRunResultDto {
        final_output: outcome.final_output,
        converged: outcome.converged,
        rounds: outcome.rounds,
    })
}

fn map_orchestrator_error(e: OrchestratorError) -> IpcError {
    match e {
        OrchestratorError::MemberNotFound { .. } => {
            IpcError::new("team.member_missing", e.to_string())
        }
        OrchestratorError::InvalidTeam(_) => IpcError::new("team.invalid_config", e.to_string()),
        other => IpcError::new("team.run_failed", other.to_string()),
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TeamRunResultDto {
    pub final_output: String,
    pub converged: bool,
    pub rounds: usize,
}

// ---------- integrations (SPEC bots-telemetry-m1 B4) ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationKindDto {
    FeishuBot,
    QqWebhook,
    Telemetry,
}

fn kind_to_dto(kind: IntegrationKind) -> IntegrationKindDto {
    match kind {
        IntegrationKind::FeishuBot => IntegrationKindDto::FeishuBot,
        IntegrationKind::QqWebhook => IntegrationKindDto::QqWebhook,
        IntegrationKind::Telemetry => IntegrationKindDto::Telemetry,
    }
}

fn kind_from_dto(kind: IntegrationKindDto) -> IntegrationKind {
    match kind {
        IntegrationKindDto::FeishuBot => IntegrationKind::FeishuBot,
        IntegrationKindDto::QqWebhook => IntegrationKind::QqWebhook,
        IntegrationKindDto::Telemetry => IntegrationKind::Telemetry,
    }
}

/// IPC-safe integration view. The webhook URL crosses the boundary masked
/// only (write-only field: stored raw in SQLite per SPEC D5, never read
/// back); the secret never leaves the process at all.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationDto {
    pub id: String,
    pub name: String,
    pub kind: IntegrationKindDto,
    pub webhook_url_masked: String,
    pub events: Vec<String>,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

impl IntegrationDto {
    fn from_entity(integration: Integration) -> Self {
        let raw = webhook_url_of(&integration);
        Self {
            id: integration.id,
            name: integration.name,
            kind: kind_to_dto(integration.kind),
            webhook_url_masked: mask_webhook_url(&raw),
            events: integration.events,
            enabled: integration.enabled,
            created_at: integration.created_at,
            updated_at: integration.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationInput {
    pub name: String,
    pub kind: IntegrationKindDto,
    pub webhook_url: String,
    /// Feishu signing secret; persisted into `config.secret` for
    /// `feishu_bot` rows only.
    pub secret: Option<String>,
    /// Optional extra headers for generic webhook/telemetry endpoints.
    pub headers: Option<BTreeMap<String, String>>,
    /// Trigger-topic whitelist (empty = all domain topics on dispatch).
    pub events: Vec<String>,
    pub enabled: bool,
}

impl IntegrationInput {
    fn validate(&self) -> Result<(), IpcError> {
        if self.name.trim().is_empty() {
            return Err(IpcError::with_details(
                "integration.invalid",
                "integration name must not be empty",
                serde_json::json!({ "reason": "name" }),
            ));
        }
        if !(self.webhook_url.starts_with("http://") || self.webhook_url.starts_with("https://")) {
            return Err(IpcError::with_details(
                "integration.invalid",
                "webhook url must start with http:// or https://",
                serde_json::json!({ "reason": "webhook_url" }),
            ));
        }
        Ok(())
    }

    fn build_config(&self) -> serde_json::Value {
        let mut config = serde_json::json!({ "webhook_url": self.webhook_url });
        if matches!(self.kind, IntegrationKindDto::FeishuBot) {
            if let Some(secret) = &self.secret {
                config["secret"] = serde_json::json!(secret);
            }
        }
        if let Some(headers) = &self.headers {
            config["headers"] = serde_json::json!(headers);
        }
        config
    }
}

/// Bumps the notifier's integration generation so the dispatcher
/// re-materializes its sinks without an app restart. The send result is
/// deliberately ignored: it fails only when the notifier already dropped its
/// receiver (dispatcher exited / process shutting down) — there is nothing
/// left to reload, and the DB write itself has already succeeded.
fn notify_integrations_changed(state: &AppState) {
    let next = state.integrations_reload_tx.borrow().wrapping_add(1);
    let _ = state.integrations_reload_tx.send(next);
}

/// `name` is the idempotency key: an existing integration with the same
/// name is updated in place (keeping its `id`/`created_at`), otherwise
/// inserted fresh with a uuid v7 id. The response masks the URL like every
/// other outbound surface (SPEC D5).
pub async fn impl_upsert_integration(
    state: &AppState,
    input: IntegrationInput,
) -> Result<IntegrationDto, IpcError> {
    input.validate()?;
    let path = state.db_path.clone();
    let entity = tokio::task::spawn_blocking(move || -> Result<Integration, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let now = now_ms();
        let existing = repos::integrations::list(&db.0)?
            .into_iter()
            .find(|i| i.name == input.name);
        let config = input.build_config();
        match existing {
            Some(mut prev) => {
                prev.kind = kind_from_dto(input.kind);
                prev.config = config;
                prev.events = input.events;
                prev.enabled = input.enabled;
                prev.updated_at = now;
                repos::integrations::update(&db.0, &prev)?;
                Ok(prev)
            }
            None => {
                let fresh = Integration {
                    id: nuomi_core::domain::new_id(),
                    name: input.name,
                    kind: kind_from_dto(input.kind),
                    config,
                    events: input.events,
                    enabled: input.enabled,
                    created_at: now,
                    updated_at: now,
                };
                repos::integrations::insert(&db.0, &fresh)?;
                Ok(fresh)
            }
        }
    })
    .await??;
    notify_integrations_changed(state);
    Ok(IntegrationDto::from_entity(entity))
}

pub async fn impl_list_integrations(state: &AppState) -> Result<Vec<IntegrationDto>, IpcError> {
    let path = state.db_path.clone();
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<Integration>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(repos::integrations::list(&db.0)?)
    })
    .await??;
    Ok(rows.into_iter().map(IntegrationDto::from_entity).collect())
}

pub async fn impl_delete_integration(
    state: &AppState,
    integration_id: String,
) -> Result<(), IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        if !repos::integrations::delete(&db.0, &integration_id)? {
            return Err(IpcError::new(
                "integration.not_found",
                format!("integration#{integration_id} not found"),
            ));
        }
        Ok(())
    })
    .await??;
    notify_integrations_changed(state);
    Ok(())
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TestIntegrationDto {
    pub ok: bool,
    pub error: Option<String>,
}

/// Budget for the `test_integration` probe; mirrors the sink-level HTTP
/// timeout so a wedged endpoint surfaces as `ok:false`, never a hang.
const INTEGRATION_TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Sends ("nuomi test", "integration check") through the row's sink inside
/// the timeout budget. Failures come back as `ok:false` results — the only
/// IPC error is an unknown id — and the error text is scrubbed of the full
/// webhook URL (masked form only, SPEC D5).
pub async fn impl_test_integration(
    state: &AppState,
    integration_id: String,
) -> Result<TestIntegrationDto, IpcError> {
    let path = state.db_path.clone();
    let row = tokio::task::spawn_blocking(move || -> Result<Integration, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        match repos::integrations::get(&db.0, &integration_id) {
            Ok(row) => Ok(row),
            Err(StoreError::NotFound { .. }) => Err(IpcError::new(
                "integration.not_found",
                format!("integration#{integration_id} not found"),
            )),
            Err(e) => Err(e.into()),
        }
    })
    .await??;

    let sink = build_shell_sink(&row)?;
    let raw_url = webhook_url_of(&row);
    let outcome = tokio::time::timeout(
        INTEGRATION_TEST_TIMEOUT,
        sink.send("nuomi test", "integration check"),
    )
    .await;
    match outcome {
        Ok(Ok(())) => Ok(TestIntegrationDto {
            ok: true,
            error: None,
        }),
        Ok(Err(e)) => Ok(TestIntegrationDto {
            ok: false,
            error: Some(redact_url(&e.to_string(), &raw_url)),
        }),
        Err(_elapsed) => Ok(TestIntegrationDto {
            ok: false,
            error: Some(format!(
                "timed out after {}s",
                INTEGRATION_TEST_TIMEOUT.as_secs()
            )),
        }),
    }
}

/// Shell-side reconstruction of the core sink dispatch (the core builder is
/// private): feishu rows carry the optional signing secret from
/// `config.secret`; generic rows carry optional extra headers.
fn build_shell_sink(row: &Integration) -> Result<Arc<dyn OutboundSink>, IpcError> {
    use nuomi_core::integrations::feishu::FeishuSink;
    use nuomi_core::integrations::webhook::GenericWebhookSink;

    let url = row
        .config
        .get("webhook_url")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            IpcError::with_details(
                "integration.invalid",
                "integration has no webhook_url configured",
                serde_json::json!({ "reason": "webhook_url" }),
            )
        })?;
    Ok(match row.kind {
        IntegrationKind::FeishuBot => Arc::new(FeishuSink::new(
            url,
            row.config
                .get("secret")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        )),
        IntegrationKind::QqWebhook | IntegrationKind::Telemetry => {
            let mut headers = BTreeMap::new();
            if let Some(obj) = row
                .config
                .get("headers")
                .and_then(serde_json::Value::as_object)
            {
                for (key, value) in obj {
                    if let Some(s) = value.as_str() {
                        headers.insert(key.clone(), s.to_string());
                    }
                }
            }
            Arc::new(GenericWebhookSink::new(row.kind, url, headers))
        }
    })
}

fn webhook_url_of(integration: &Integration) -> String {
    integration
        .config
        .get("webhook_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Keeps the full webhook URL out of IPC-boundary error text (SPEC D5).
fn redact_url(message: &str, raw_url: &str) -> String {
    if raw_url.is_empty() {
        return message.to_string();
    }
    message.replace(raw_url, &mask_webhook_url(raw_url))
}

/// Masks a webhook URL across the IPC boundary (SPEC D5): `scheme://host`
/// is kept verbatim, every path segment collapses to its first character
/// plus `…`, and the last segment additionally keeps its final four
/// characters — e.g. `https://open.feishu.cn/open-apis/bot/v2/hook/a1b2c3d4x9z8`
/// → `https://open.feishu.cn/o…/b…/v…/h…/a…x9z8`.
fn mask_webhook_url(raw: &str) -> String {
    let Some((scheme, rest)) = raw.split_once("://") else {
        return String::new();
    };
    let (host, path) = match rest.split_once('/') {
        Some((host, path)) => (host, Some(path)),
        None => (rest, None),
    };
    let base = format!("{scheme}://{host}");
    let Some(path) = path else {
        return base;
    };
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return base;
    }
    let last = segments.len() - 1;
    let mut masked = base;
    for (idx, segment) in segments.iter().enumerate() {
        masked.push('/');
        let chars: Vec<char> = segment.chars().collect();
        let Some(first) = chars.first() else {
            continue;
        };
        masked.push(*first);
        masked.push('…');
        // Keep the tail-4 only when it cannot overlap the leading char.
        if idx == last && chars.len() > 6 {
            let tail: String = chars[chars.len() - 4..].iter().collect();
            masked.push_str(&tail);
        }
    }
    masked
}

// ---------- DTOs ----------

fn now_ms() -> i64 {
    nuomi_core::domain::now_ms()
}

fn append_domain_event(
    conn: &rusqlite::Connection,
    topic: &str,
    payload: serde_json::Value,
    at: i64,
) -> Result<(), IpcError> {
    repos::events::append(conn, "domain", "global", topic, &payload, at)?;
    Ok(())
}

fn task_id_payload(id: &str) -> serde_json::Value {
    serde_json::json!({ "taskId": id })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocolDto {
    OpenAiCompatible,
    AnthropicCompatible,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDto {
    pub id: String,
    pub name: String,
    pub protocol: ProviderProtocolDto,
    pub base_url: String,
    pub has_key: bool,
    pub capabilities: Vec<String>,
    pub is_master: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunDto {
    pub id: String,
    pub task_id: String,
    pub session_id: String,
    pub status: String,
    pub heartbeat_at: i64,
}

impl From<nuomi_core::domain::Run> for RunDto {
    fn from(r: nuomi_core::domain::Run) -> Self {
        Self {
            id: r.id,
            task_id: r.task_id,
            session_id: r.session_id,
            status: r.status.as_str().to_string(),
            heartbeat_at: r.heartbeat_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalDto {
    pub id: String,
    pub run_id: String,
    pub tool_name: String,
    pub arguments_json: String,
}

impl From<nuomi_core::domain::Approval> for ApprovalDto {
    fn from(a: nuomi_core::domain::Approval) -> Self {
        Self {
            id: a.id,
            run_id: a.run_id,
            tool_name: a.tool_name,
            arguments_json: a.arguments_json,
        }
    }
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FileEntryDto {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusDto {
    pub index_status: String,
    pub worktree_status: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitDto {
    pub hash: String,
    pub subject: String,
    pub author: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktreeDto {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub is_current: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleDto {
    pub id: String,
    pub name: String,
    pub cron_expr: String,
    pub task_title: String,
    pub enabled: bool,
    pub next_trigger_at: Option<i64>,
}

impl From<Schedule> for ScheduleDto {
    fn from(s: Schedule) -> Self {
        Self {
            id: s.id,
            name: s.name,
            cron_expr: s.cron_expr,
            task_title: s.task_title,
            enabled: s.enabled,
            next_trigger_at: s.next_trigger_at,
        }
    }
}

// ---------- workspace switch ----------

pub async fn impl_get_workspace(state: &AppState) -> Result<String, IpcError> {
    Ok(state.current_workspace().to_string_lossy().to_string())
}

pub async fn impl_set_workspace(state: &AppState, path: String) -> Result<String, IpcError> {
    let previous = state
        .switch_workspace(PathBuf::from(&path))
        .map_err(IpcError::from)?;
    repos::events::append(
        &Db::open(&state.db_path)?.0,
        "domain",
        "global",
        "workspace.switched",
        &serde_json::json!({ "path": path, "previous": previous.to_string_lossy() }),
        nuomi_core::domain::now_ms(),
    )?;
    Ok(previous.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::mask_webhook_url;

    /// SPEC D5 shape: scheme+host kept, per-segment first char, tail-4 of
    /// the last segment; the raw URL is never recoverable from the result.
    #[test]
    fn webhook_url_masking_keeps_host_and_truncates_segments() {
        let raw = "https://open.feishu.cn/open-apis/bot/v2/hook/a1b2c3d4e5f6x9z8";
        let masked = mask_webhook_url(raw);
        assert_eq!(masked, "https://open.feishu.cn/o…/b…/v…/h…/a…x9z8");
        assert!(!masked.contains("open-apis"));
        assert!(!masked.contains("a1b2c3d4e5f6"));

        // Short last segment: no overlapping tail duplication.
        assert_eq!(
            mask_webhook_url("https://qq.test/gateway/hook"),
            "https://qq.test/g…/h…"
        );
        // No path: host only.
        assert_eq!(
            mask_webhook_url("http://localhost:9000"),
            "http://localhost:9000"
        );
        // Trailing slash only: still just the base.
        assert_eq!(mask_webhook_url("https://x.test/"), "https://x.test");
        // Non-http scheme (pre-validation) still masks deterministically.
        assert_eq!(
            mask_webhook_url("ftp://f.test/a1b2c3d4"),
            "ftp://f.test/a…c3d4"
        );
        // Missing scheme: nothing sensible to reveal.
        assert_eq!(mask_webhook_url("not-a-url"), "");
    }
}
