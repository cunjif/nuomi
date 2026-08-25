//! All IPC commands, grouped by surface. Thin validation + forwarding only.
//!
//! Every command: `#[tauri::command] #[specta::specta]`, returns
//! `Result<T, IpcError>`, snake_case verb naming (ipc-contract rules).
//! The `#[tauri::command]`/invoke wiring is applied in `lib.rs`; the inner
//! `impl_*` free functions are testable without a Tauri runtime.

use serde::Serialize;
use std::path::PathBuf;

use nuomi_core::domain::run_state::{ApprovalOutcome, RunEvent};
use nuomi_core::domain::{
    EventRecord, ProviderConfig, ProviderProtocol, RunState, Schedule, Task, TaskStatus,
};
use nuomi_core::evolution::research::{
    online_authorized as core_online_authorized, set_online_authorized,
};
use nuomi_core::plugins::approval_gate;
use nuomi_core::services::parse_schedule;
use nuomi_core::store::{migrations, repos, Db};

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
        dispatch_run(state, task_id).await?;
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

/// Applies one state-machine step with the persisted-event-first iron rule.
pub(crate) fn transition_run(
    conn: &rusqlite::Connection,
    run_id: &str,
    expected: RunState,
    ev: RunEvent,
) -> Result<RunState, IpcError> {
    let next = expected
        .transition(ev)
        .map_err(|e| IpcError::new("domain.invalid", e.to_string()))?;
    let now = now_ms();
    repos::events::append(
        conn,
        "run",
        run_id,
        "state_changed",
        &serde_json::json!({ "from": expected.as_str(), "to": next.as_str() }),
        now,
    )?;
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
