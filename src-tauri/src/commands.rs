//! All IPC commands, grouped by surface. Thin validation + forwarding only.
//!
//! Every command: `#[tauri::command] #[specta::specta]`, returns
//! `Result<T, IpcError>`, snake_case verb naming (ipc-contract rules).
//! The `#[tauri::command]`/invoke wiring is applied in `lib.rs`; the inner
//! `impl_*` free functions are testable without a Tauri runtime.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tauri::Manager;

use nuomi_core::domain::run_state::{ApprovalOutcome, RunEvent};
use nuomi_core::domain::{
    AgentProfile, AgentRefKind, CliFlavor, ConversationKind, EventRecord, Integration,
    IntegrationKind, ProviderConfig, ProviderProtocol, Role, RunState, Schedule,
    ScheduleSessionMode, ScheduleTargetKind, Session, SessionCliHandle, Task, TaskStatus, Team,
    TeamTopology, WhiteBoardNote,
};
use nuomi_core::evolution::research::{
    online_authorized as core_online_authorized, set_online_authorized,
};
use nuomi_core::integrations::OutboundSink;
use nuomi_core::orchestrator::OrchestratorError;
use nuomi_core::plugins::LoopRunResult;
use nuomi_core::plugins::approval_gate;
use nuomi_core::services::capability_router::RouteOutcome;
use nuomi_core::services::parse_schedule;
use nuomi_core::services::SeedReport;
use nuomi_core::services::{
    check_provider_refs, check_role_refs, delete_and_nullify_provider_refs,
    delete_and_nullify_role_refs, detect_missing_provider, emit_env_fallback,
    emit_materialize_warnings, emit_materialized, emit_provider_missing, emit_role_applied,
    materialize_single_role, resolve_participants, run_team as core_run_team, MissingProviderHint,
    ResolvedAgent, SingleRoleContext, TeamRunOutcome,
};
use nuomi_core::store::{migrations, repos, Db, StoreError};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{ChildStderr, ChildStdout};
use tokio_util::sync::CancellationToken;

use crate::ipc_error::IpcError;
use crate::state::{join_err, AppState};

/// Returns the currently focused workspace id, if any. Used to bind runs to
/// their workspace context for parallel-run isolation (task 7.1).
fn focused_workspace_id(db_path: &str) -> Option<String> {
    let db = Db::open(db_path).ok()?;
    migrations::run(&db.0).ok()?;
    repos::workspace_open_state::find_focused(&db.0)
        .ok()?
        .map(|r| r.workspace_id)
}

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

/// Persists the CLI Agent's own session id back to `session_cli_handles`
/// (ADR 0012 D8). Called after each conversation turn; no-op when the
/// provider was not a CLI agent or did not report a session id.
async fn upsert_cli_handle(
    db_path: &str,
    session_id: &str,
    cli_handle: &Option<(String, Option<String>, Option<String>, Option<usize>)>,
    result: &LoopRunResult,
) -> Result<(), IpcError> {
    let Some((role_agent_id, agent_profile_id, _, _)) = cli_handle else {
        return Ok(());
    };
    let Some(ref cli_session_id) = result.cli_session_id else {
        return Ok(());
    };
    let sid = session_id.to_string();
    let role_id = role_agent_id.clone();
    let profile_id = agent_profile_id.clone().unwrap_or_default();
    let cli_id = cli_session_id.clone();
    let path = db_path.to_string();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        repos::session_cli_handles::upsert(
            &db.0,
            &SessionCliHandle {
                session_id: sid,
                role_agent_id: role_id,
                agent_profile_id: profile_id,
                cli_session_id: Some(cli_id),
                updated_at: now_ms(),
            },
        )?;
        Ok(())
    })
    .await
    .map_err(join_err)??;
    Ok(())
}

/// Runs one conversation turn against an explicit session: history is
/// loaded from (and persisted to) that session, so two conversations can
/// run concurrently and a message never lands in the wrong transcript.
///
/// The run registers a cooperative cancellation token under its session id
/// so `/stop` and the background tray can abort it; the loop returns at the
/// next step boundary and whatever it produced is persisted.
pub async fn run_conversation_turn(
    state: &AppState,
    session_id: &str,
    text: &str,
) -> Result<RunResultDto, IpcError> {
    if session_id.trim().is_empty() {
        return Err(IpcError::new(
            "session.invalid_id",
            "session id must not be empty",
        ));
    }
    let token = state.session_cancels.begin(session_id);

    // Dispatch based on team_id (D1 / D3). Wrapped in a block so that
    // `finish` is always called — even on error paths (`?` inside the block
    // returns from the block, not the function).
    let result = {
        // Load session row + resolve participants (ADR 0013: multi-participant
        // group chat). Wrapped in a block so that `finish` is always called.
        let db_path = state.db_path.clone();
        let sid_load = session_id.to_string();
        let (session, participants, missing_hint, cli_handle) =
            tokio::task::spawn_blocking(
                move || -> Result<
                    (
                        Session,
                        Vec<ResolvedAgent>,
                        Option<MissingProviderHint>,
                        Option<(String, Option<String>, Option<String>, Option<usize>)>,
                    ),
                    IpcError,
                > {
                    let db = Db::open(&db_path)?;
                    let session = repos::sessions::get(&db.0, &sid_load)
                        .map_err(|_| IpcError::new("session.not_found", "session not found"))?;
                    let participants =
                        resolve_participants(&db.0, &session.id).ok().unwrap_or_default();
                    tracing::info!(
                        session_id = %sid_load,
                        count = participants.len(),
                        ids = ?participants.iter().map(|p| (&p.kind, &p.id)).collect::<Vec<_>>(),
                        "resolved participants",
                    );
                    let resolved = participants.first().cloned();
                    // Detect missing provider for Role-bound agents (AC6 runtime).
                    let missing_hint = match &resolved {
                        Some(r) if matches!(r.kind, AgentRefKind::Role) => {
                            repos::roles::get(&db.0, &r.id)
                                .ok()
                                .and_then(|role| detect_missing_provider(&role))
                        }
                        _ => None,
                    };
                    // Look up CLI session handle for Role agents (ADR 0012 D8).
                    // Detect binding change: if the handle's agent_profile_id
                    // differs from the current binding, clear the handle and
                    // pass truncated context (ADR 0012 D7).
                    let cli_handle = match &resolved {
                        Some(r) if matches!(r.kind, AgentRefKind::Role) => {
                            let current_profile_id = repos::roles::get(&db.0, &r.id)
                                .ok()
                                .and_then(|role| {
                                    role.params
                                        .get("agent_profile_id")
                                        .and_then(serde_json::Value::as_str)
                                        .map(|s| s.to_string())
                                });
                            let existing_handle = repos::session_cli_handles::get(
                                &db.0,
                                &sid_load,
                                &r.id,
                            )
                            .ok()
                            .flatten();
                            let (external_session_id, max_history_chars) =
                                match &existing_handle {
                                    Some(h) if h.agent_profile_id
                                        != current_profile_id.clone().unwrap_or_default() =>
                                    {
                                        // Binding changed: clear old handle,
                                        // start fresh with truncated context.
                                        let _ = repos::session_cli_handles::clear(
                                            &db.0,
                                            &sid_load,
                                            &r.id,
                                        );
                                        let handover_tokens = repos::settings::get(
                                            &db.0,
                                            "cli_context_handover_tokens",
                                        )
                                        .ok()
                                        .flatten()
                                        .and_then(|s| s.parse::<usize>().ok())
                                        .unwrap_or(65536);
                                        // tokens → chars (≈4 chars/token).
                                        (None, Some(handover_tokens * 4))
                                    }
                                    Some(h) => (h.cli_session_id.clone(), None),
                                    None => (None, None),
                                };
                            Some((
                                r.id.clone(),
                                current_profile_id,
                                external_session_id,
                                max_history_chars,
                            ))
                        }
                        _ => None,
                    };
                    Ok((session, participants, missing_hint, cli_handle))
                },
            )
            .await
            .map_err(join_err)??;

        // Emit provider.missing hint if detected (non-blocking warning).
        if let Some(ref hint) = missing_hint {
            let bus = state.kernel.context().bus();
            let db_path = state.db_path.clone();
            emit_provider_missing(&bus, &db_path, session_id, &hint.role_id, &hint.role_name)
                .await
                .ok();
        }

        if let Some(ref team_id) = session.team_id {
            // Team path: dispatch to core_run_team (群聊拓扑: Selector +
            // Handoff + WhiteBoard). 续传时按 team_id 有无分派，未绑定的
            // 既有会话走单 Role/Provider 路径 (D3).
            let db_path = state.db_path.clone();
            let secrets = state.secrets.clone();
            let cwd = Some(state.current_workspace());
            let bus = state.kernel.context().bus();
            let ws_id = focused_workspace_id(&db_path);
            let outcome = core_run_team(db_path, Some(bus), team_id, session_id, text, secrets, cwd, ws_id)
                .await
                .map_err(|e| IpcError::new("team.run_failed", &e.to_string()))?;
            LoopRunResult {
                final_text: outcome.final_output,
                steps: outcome.rounds,
                truncated: !outcome.converged,
                transcript: Vec::new(),
                cli_session_id: None,
            }
        } else if participants.len() > 1 {
            // ADR 0013: IM group chat — Selector mode. First participant is
            // the Selector; it picks which of the remaining agents should
            // respond to the user message.
            run_selector_turn(state, session_id, text, &token, &participants).await?
        } else {
            // Single role path: materialize from DB config (D1).
            let resolved = participants.first().cloned();
            let db_path = state.db_path.clone();
            let secrets = state.secrets.clone();
            let cwd = Some(state.current_workspace());
            let ctx = materialize_single_role(db_path, secrets, cwd, resolved.as_ref()).await?;
            let external_session_id =
                cli_handle.as_ref().and_then(|(_, _, sid, _)| sid.clone());
            let max_history_chars =
                cli_handle.as_ref().and_then(|(_, _, _, m)| *m);

            match ctx {
                SingleRoleContext::Materialized {
                    provider,
                    model,
                    overlay,
                    warnings,
                } => {
                    let provider_id = provider.id().to_string();
                    let bus = state.kernel.context().bus();
                    let db_path = state.db_path.clone();
                    emit_materialized(&bus, &db_path, session_id, &provider_id)
                        .await
                        .ok();
                    // AC7: materialize skip+warnings → EventRecord (not hard error).
                    if !warnings.is_empty() {
                        let db_path = state.db_path.clone();
                        emit_materialize_warnings(&bus, &db_path, session_id, &warnings)
                            .await
                            .ok();
                    }
                    if let (Some(ref ov), Some(ref r)) = (&overlay, &resolved) {
                        if matches!(r.kind, AgentRefKind::Role) {
                            let db_path = state.db_path.clone();
                            emit_role_applied(&bus, &db_path, session_id, &r.id, ov)
                                .await
                                .ok();
                        }
                    }
                    let result = state
                        .kernel
                        .run_task_in_session(
                            session_id,
                            text,
                            Some(token.clone()),
                            Some(provider),
                            Some(model),
                            overlay,
                            external_session_id,
                            max_history_chars,
                        )
                        .await?;
                    // P1-6: CLI handle persistence is best-effort. The
                    // reply is already persisted at this point; a failure
                    // here (e.g. DB write error) should not surface as an
                    // IPC error — that would make the frontend retry and
                    // duplicate the message.
                    if let Err(e) = upsert_cli_handle(state.db_path.as_ref(), session_id, &cli_handle, &result).await {
                        tracing::warn!(error = %e, session_id, "failed to persist cli handle; reply is already saved");
                    }
                    result
                }
                SingleRoleContext::EnvFallback => {
                    let bus = state.kernel.context().bus();
                    let db_path = state.db_path.clone();
                    emit_env_fallback(&bus, &db_path, session_id)
                        .await
                        .ok();
                    let result = state
                        .kernel
                        .run_task_in_session(
                            session_id,
                            text,
                            Some(token.clone()),
                            None,
                            None,
                            None,
                            external_session_id,
                            max_history_chars,
                        )
                        .await?;
                    // P1-6: same best-effort CLI handle persistence.
                    if let Err(e) = upsert_cli_handle(state.db_path.as_ref(), session_id, &cli_handle, &result).await {
                        tracing::warn!(error = %e, session_id, "failed to persist cli handle; reply is already saved");
                    }
                    result
                }
            }
        }
    };

    // A cancelled run was already deregistered by the canceller.
    if !token.is_cancelled() {
        state.session_cancels.finish(session_id);
    }
    Ok(RunResultDto {
        final_text: result.final_text,
        steps: result.steps,
        truncated: result.truncated,
        session_id: session_id.to_string(),
    })
}

/// ADR 0013: IM group chat Selector mode. The first participant acts as
/// Selector — it receives the user message plus a list of candidate agents
/// and replies with the id of the agent who should respond. The selected
/// agent then processes the original user message.
async fn run_selector_turn(
    state: &AppState,
    session_id: &str,
    text: &str,
    token: &CancellationToken,
    participants: &[ResolvedAgent],
) -> Result<LoopRunResult, IpcError> {
    let selector = &participants[0];
    let candidates = &participants[1..];

    tracing::info!(
        session_id,
        selector_id = %selector.id,
        selector_name = %selector.name,
        candidate_count = candidates.len(),
        "group chat: selector mode",
    );

    // 1. Materialize Selector agent.
    let db_path = state.db_path.clone();
    let secrets = state.secrets.clone();
    let cwd = Some(state.current_workspace());
    let selector_ctx = materialize_single_role(db_path, secrets, cwd, Some(selector))
        .await
        .map_err(|e| IpcError::new("selector.materialize_failed", &e.to_string()))?;

    // 2. Build selector prompt with candidate list.
    let candidate_list = candidates
        .iter()
        .map(|p| format!("- id: {}, name: {}", p.id, p.name))
        .collect::<Vec<_>>()
        .join("\n");
    let selector_prompt = format!(
        "You are a selector. Choose the best agent to respond to the user message.\n\n\
         Available agents:\n{}\n\n\
         User message: {}\n\n\
         Reply with ONLY the id of the chosen agent.",
        candidate_list, text
    );

    // 3. Run Selector to pick the responding agent.
    let selector_result =
        run_single_role_context(state, session_id, &selector_prompt, token, selector_ctx).await?;

    tracing::info!(
        session_id,
        selector_reply = %selector_result.final_text,
        "selector responded",
    );

    // 4. Parse selector reply — find the candidate whose id appears in the reply.
    let selected = candidates
        .iter()
        .find(|p| selector_result.final_text.contains(&p.id))
        .or_else(|| candidates.first())
        .cloned();

    tracing::info!(
        session_id,
        selected_id = ?selected.as_ref().map(|p| &p.id),
        selected_name = ?selected.as_ref().map(|p| &p.name),
        "selected agent for response",
    );

    // 5. Materialize and run the selected agent with the original user message.
    let db_path = state.db_path.clone();
    let secrets = state.secrets.clone();
    let cwd = Some(state.current_workspace());
    let selected_ctx = materialize_single_role(db_path, secrets, cwd, selected.as_ref())
        .await
        .map_err(|e| IpcError::new("selected.materialize_failed", &e.to_string()))?;

    run_single_role_context(state, session_id, text, token, selected_ctx).await
}

/// Helper: run a SingleRoleContext (Materialized or EnvFallback) through the
/// kernel. Emits materialized/env_fallback events for UI diagnostics.
async fn run_single_role_context(
    state: &AppState,
    session_id: &str,
    text: &str,
    token: &CancellationToken,
    ctx: SingleRoleContext,
) -> Result<LoopRunResult, IpcError> {
    match ctx {
        SingleRoleContext::Materialized {
            provider,
            model,
            overlay,
            warnings,
        } => {
            let provider_id = provider.id().to_string();
            let bus = state.kernel.context().bus();
            let db_path = state.db_path.clone();
            emit_materialized(&bus, &db_path, session_id, &provider_id)
                .await
                .ok();
            if !warnings.is_empty() {
                let db_path = state.db_path.clone();
                emit_materialize_warnings(&bus, &db_path, session_id, &warnings)
                    .await
                    .ok();
            }
            state
                .kernel
                .run_task_in_session(
                    session_id,
                    text,
                    Some(token.clone()),
                    Some(provider),
                    Some(model),
                    overlay,
                    None,
                    None,
                )
                .await
                .map_err(|e| IpcError::new("agent.run_failed", &e.to_string()))
        }
        SingleRoleContext::EnvFallback => {
            let bus = state.kernel.context().bus();
            let db_path = state.db_path.clone();
            emit_env_fallback(&bus, &db_path, session_id).await.ok();
            state
                .kernel
                .run_task_in_session(
                    session_id,
                    text,
                    Some(token.clone()),
                    None,
                    None,
                    None,
                    None,
                    None,
                )
                .await
                .map_err(|e| IpcError::new("agent.run_failed", &e.to_string()))
        }
    }
}

pub async fn impl_submit_task(
    app_handle: tauri::AppHandle,
    state: &AppState,
    session_id: String,
    input: String,
) -> Result<RunResultDto, IpcError> {
    // P1-1: use try_set_busy so we don't clobber a running turn. If the
    // agent is already busy, the frontend should route through
    // `enqueue_message` instead of `submit_task`.
    let acquired = {
        let db_path = state.db_path.clone();
        let sid = session_id.clone();
        tokio::task::spawn_blocking(move || -> Result<bool, StoreError> {
            let db = Db::open(&db_path)?;
            repos::sessions::try_set_busy(&db.0, &sid)
        })
        .await
        .map_err(join_err)??
    };
    if !acquired {
        return Err(IpcError::new(
            "session.agent_busy",
            "agent is already processing a turn; use enqueue_message instead",
        ));
    }
    let result = run_conversation_turn(state, &session_id, &input).await;
    // Release the busy lock and notify the frontend.
    release_busy_and_emit(state, &session_id).await;
    // P0-5: lost-wakeup — a producer may have enqueued while this direct
    // turn held the busy lock (impl_enqueue_message only spawns a drainer
    // when IT acquires the lock). Re-check the queue and start a drainer
    // if any messages were orphaned by this turn.
    maybe_start_drainer(app_handle, state, &session_id).await;
    result
}

/// ADR 0015: Marks the session's agent busy/idle in the DB.
async fn set_agent_busy(state: &AppState, session_id: &str, busy: bool) {
    let db_path = state.db_path.clone();
    let sid = session_id.to_string();
    let _ = tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
        let db = Db::open(&db_path)?;
        repos::sessions::set_busy(&db.0, &sid, busy)
    })
    .await;
}

/// ADR 0015: Emits `session.turn_end` with the remaining queue length so
/// the frontend can refresh the queue list. Does NOT touch the busy flag
/// — used by the drainer between turns to keep the lock held (P0-2 fix).
async fn emit_turn_end(state: &AppState, session_id: &str) {
    let db_path = state.db_path.clone();
    let sid = session_id.to_string();
    let remaining = tokio::task::spawn_blocking(move || -> Result<i64, StoreError> {
        let db = Db::open(&db_path)?;
        repos::message_queue::count_queued(&db.0, &sid)
    })
    .await
    .ok()
    .and_then(|r| r.ok())
    .unwrap_or(0);
    state.kernel.context().publish(nuomi_core::harness::Event::new(
        "session.turn_end",
        serde_json::json!({ "sessionId": session_id, "queueRemaining": remaining }),
    ));
}

/// ADR 0015: Releases the busy lock (sets `agent_busy = 0`) and emits
/// `session.turn_end`. Used by `impl_submit_task` after a direct (non-
/// queued) turn completes.
async fn release_busy_and_emit(state: &AppState, session_id: &str) {
    let db_path = state.db_path.clone();
    let sid = session_id.to_string();
    let remaining = tokio::task::spawn_blocking(move || -> Result<i64, StoreError> {
        let db = Db::open(&db_path)?;
        repos::sessions::set_busy(&db.0, &sid, false)?;
        repos::message_queue::count_queued(&db.0, &sid)
    })
    .await
    .ok()
    .and_then(|r| r.ok())
    .unwrap_or(0);
    state.kernel.context().publish(nuomi_core::harness::Event::new(
        "session.turn_end",
        serde_json::json!({ "sessionId": session_id, "queueRemaining": remaining }),
    ));
}

/// P0-5: Lost-wakeup recovery for direct turns. A producer may have enqueued
/// via `impl_enqueue_message` while a `submit_task` turn held the busy lock —
/// in that window the producer's `try_set_busy` fails and no drainer is
/// spawned. After the direct turn releases the lock, atomically re-check the
/// queue and re-acquire the lock; if both succeed, start the drainer.
async fn maybe_start_drainer(
    app_handle: tauri::AppHandle,
    state: &AppState,
    session_id: &str,
) {
    let db_path = state.db_path.clone();
    let sid = session_id.to_string();
    let reacquired = tokio::task::spawn_blocking(move || -> Result<bool, StoreError> {
        let db = Db::open(&db_path)?;
        let count = repos::message_queue::count_queued(&db.0, &sid)?;
        if count == 0 {
            return Ok(false);
        }
        repos::sessions::try_set_busy(&db.0, &sid)
    })
    .await
    .ok()
    .and_then(|r| r.ok())
    .unwrap_or(false);
    if reacquired {
        let sid = session_id.to_string();
        tokio::task::spawn(async move {
            process_queue(app_handle, sid).await;
        });
    }
}

// ---------------------------------------------------- ADR 0015: message queue

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MessageQueueItemDto {
    pub id: String,
    pub text: String,
    pub seq: i64,
    pub created_at: i64,
}

/// Enqueues a message. If the agent is idle, spawns a background loop that
/// drains the queue turn by turn. Returns immediately (does not wait for
/// the turn to complete).
pub async fn impl_enqueue_message(
    app_handle: tauri::AppHandle,
    state: &AppState,
    session_id: String,
    input: String,
) -> Result<MessageQueueItemDto, IpcError> {
    let db_path = state.db_path.clone();
    let sid = session_id.clone();
    let text = input.clone();
    let entry = tokio::task::spawn_blocking(move || -> Result<nuomi_core::domain::MessageQueueEntry, StoreError> {
        let db = Db::open(&db_path)?;
        repos::message_queue::enqueue(&db.0, &sid, &text)
    })
    .await
    .map_err(join_err)??;

    // Try to acquire the busy lock. If successful, spawn the queue drainer.
    let acquired = {
        let db_path = state.db_path.clone();
        let sid = session_id.clone();
        tokio::task::spawn_blocking(move || -> Result<bool, StoreError> {
            let db = Db::open(&db_path)?;
            repos::sessions::try_set_busy(&db.0, &sid)
        })
        .await
        .map_err(join_err)??
    };
    if acquired {
        let app_handle = app_handle.clone();
        let sid = session_id.clone();
        tokio::task::spawn(async move {
            process_queue(app_handle, sid).await;
        });
    }

    Ok(MessageQueueItemDto {
        id: entry.id,
        text: entry.text,
        seq: entry.seq,
        created_at: entry.created_at,
    })
}

/// Background queue drainer: dequeues and processes messages one by one
/// until the queue is empty, then exits (releasing the busy lock).
///
/// Invariants (P0-2/P0-3/P0-4 fixes):
/// - The busy flag is held for the *entire* drain loop, not released
///   between turns. This prevents a second drainer from spawning while
///   the first is still running.
/// - Turn errors are recorded as `failed` (not `done`) and surfaced to
///   the frontend via `session.queue_error`; the entry is not retried.
/// - Before exiting, a double-check prevents lost-wakeup: after dequeue
///   returns None we re-count, and after releasing busy we re-count +
///   re-acquire. If a producer enqueued in the window, we continue.
async fn process_queue(app_handle: tauri::AppHandle, session_id: String) {
    loop {
        let state = app_handle.state::<AppState>();
        // Dequeue the oldest queued message.
        let entry = {
            let db_path = state.db_path.clone();
            let sid = session_id.clone();
            tokio::task::spawn_blocking(move || -> Result<Option<nuomi_core::domain::MessageQueueEntry>, StoreError> {
                let db = Db::open(&db_path)?;
                repos::message_queue::dequeue(&db.0, &sid)
            })
            .await
            .ok()
            .and_then(|r| r.ok())
            .flatten()
        };
        match entry {
            Some(e) => {
                let turn_result = run_conversation_turn(&state, &session_id, &e.text).await;
                match turn_result {
                    Ok(_) => {
                        // Success — mark done.
                        let db_path = state.db_path.clone();
                        let id = e.id.clone();
                        let _ = tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
                            let db = Db::open(&db_path)?;
                            repos::message_queue::mark_done(&db.0, &id)
                        })
                        .await;
                    }
                    Err(err) => {
                        // P0-3: turn failed — mark failed (not done) and
                        // surface the error to the frontend so the user
                        // can see/retry. Do not swallow.
                        let db_path = state.db_path.clone();
                        let id = e.id.clone();
                        let _ = tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
                            let db = Db::open(&db_path)?;
                            repos::message_queue::mark_failed(&db.0, &id)
                        })
                        .await;
                        let err_msg = match &err {
                            crate::ipc_error::IpcError::Generic { message, .. } => message.clone(),
                        };
                        state.kernel.context().publish(nuomi_core::harness::Event::new(
                            "session.queue_error",
                            serde_json::json!({
                                "sessionId": session_id,
                                "queueId": e.id,
                                "text": e.text,
                                "error": err_msg,
                            }),
                        ));
                    }
                }
                // P0-2: emit turn_end so the frontend refreshes the queue
                // list, but do NOT release the busy flag here — we keep
                // the lock for the next iteration so no second drainer can
                // spawn.
                emit_turn_end(&state, &session_id).await;
            }
            None => {
                // P0-4: lost-wakeup double-check. After dequeue returns
                // None, a producer may have enqueued in the window before
                // we release busy. Sequence:
                //   1. count_queued → if >0, a producer raced ahead; loop.
                //   2. release busy.
                //   3. count_queued again → if >0, a producer enqueued
                //      after our count but before set_busy(0); try to
                //      re-acquire and loop. Otherwise exit.
                let state = app_handle.state::<AppState>();
                let count = {
                    let db_path = state.db_path.clone();
                    let sid = session_id.clone();
                    tokio::task::spawn_blocking(move || -> Result<i64, StoreError> {
                        let db = Db::open(&db_path)?;
                        repos::message_queue::count_queued(&db.0, &sid)
                    })
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .unwrap_or(0)
                };
                if count > 0 {
                    continue;
                }
                // Release busy, then re-check + re-acquire.
                set_agent_busy(&state, &session_id, false).await;
                let reacquired = {
                    let db_path = state.db_path.clone();
                    let sid = session_id.clone();
                    tokio::task::spawn_blocking(move || -> Result<(bool, i64), StoreError> {
                        let db = Db::open(&db_path)?;
                        let count = repos::message_queue::count_queued(&db.0, &sid)?;
                        if count == 0 {
                            return Ok((false, 0));
                        }
                        let acquired = repos::sessions::try_set_busy(&db.0, &sid)?;
                        Ok((acquired, count))
                    })
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .unwrap_or((false, 0))
                };
                if reacquired.0 {
                    // We re-acquired the lock and there are queued
                    // messages — keep draining.
                    continue;
                }
                break;
            }
        }
    }
}

pub async fn impl_list_message_queue(
    state: &AppState,
    session_id: String,
) -> Result<Vec<MessageQueueItemDto>, IpcError> {
    let db_path = state.db_path.clone();
    let sid = session_id.clone();
    let entries = tokio::task::spawn_blocking(move || -> Result<Vec<nuomi_core::domain::MessageQueueEntry>, StoreError> {
        let db = Db::open(&db_path)?;
        repos::message_queue::list_queued(&db.0, &sid)
    })
    .await
    .map_err(join_err)??;
    Ok(entries
        .into_iter()
        .map(|e| MessageQueueItemDto {
            id: e.id,
            text: e.text,
            seq: e.seq,
            created_at: e.created_at,
        })
        .collect())
}

pub async fn impl_cancel_message_queue_item(
    state: &AppState,
    id: String,
) -> Result<(), IpcError> {
    let db_path = state.db_path.clone();
    let qid = id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), StoreError> {
        let db = Db::open(&db_path)?;
        repos::message_queue::cancel(&db.0, &qid)
    })
    .await
    .map_err(join_err)??;
    Ok(())
}

pub async fn impl_clear_message_queue(
    state: &AppState,
    session_id: String,
) -> Result<usize, IpcError> {
    let db_path = state.db_path.clone();
    let sid = session_id.clone();
    let count = tokio::task::spawn_blocking(move || -> Result<usize, StoreError> {
        let db = Db::open(&db_path)?;
        repos::message_queue::clear_queued(&db.0, &sid)
    })
    .await
    .map_err(join_err)??;
    Ok(count)
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
        session_id: state.kernel.session_id().await,
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
        if let Some(ws) = repos::workspace_open_state::find_focused(&db.0)? {
            repos::tasks_runs::bind_task_workspace(&db.0, &task.id, &ws.workspace_id)?;
        }
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
pub struct AgentRefDto {
    pub kind: String,
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TodoItemDto {
    pub id: String,
    pub description: String,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDto {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub team_id: Option<String>,
    pub task_id: Option<String>,
    pub schedule_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub goal: Option<String>,
    pub main_agent_id: Option<String>,
    pub route_mode: Option<String>,
    pub whiteboard_route_mode: Option<String>,
    pub participant_agents: Vec<AgentRefDto>,
    pub todo_list: Vec<TodoItemDto>,
}

impl From<Session> for ConversationDto {
    fn from(s: Session) -> Self {
        Self {
            id: s.id,
            title: s.title,
            kind: s.kind.as_str().to_string(),
            team_id: s.team_id,
            task_id: s.task_id,
            schedule_id: s.schedule_id,
            created_at: s.created_at,
            updated_at: s.updated_at,
            goal: s.goal,
            main_agent_id: s.main_agent_id,
            route_mode: s.route_mode,
            whiteboard_route_mode: s.whiteboard_route_mode,
            participant_agents: Vec::new(),
            todo_list: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentRefInput {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommitAgentOptionDto {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiCommitResultDto {
    pub message: String,
    pub truncated: bool,
    pub agent_name: String,
    pub elapsed_ms: u64,
}

impl From<nuomi_core::services::CommitAgentOption> for CommitAgentOptionDto {
    fn from(opt: nuomi_core::services::CommitAgentOption) -> Self {
        Self {
            kind: opt.kind.as_str().to_string(),
            id: opt.id,
            name: opt.name,
            is_default: opt.is_default,
        }
    }
}

impl From<nuomi_core::services::AiCommitResult> for AiCommitResultDto {
    fn from(r: nuomi_core::services::AiCommitResult) -> Self {
        Self {
            message: r.message,
            truncated: r.truncated,
            agent_name: r.agent_name,
            elapsed_ms: r.elapsed_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ConversationInput {
    pub kind: String,
    pub title: Option<String>,
    pub agent: Option<AgentRefInput>,
    pub team_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentOptionDto {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub builtin: bool,
    pub role: Option<String>,
    pub responsibility: Option<String>,
    pub bound_model: Option<String>,
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentDetailDto {
    pub kind: String,
    pub id: String,
    pub name: String,
    pub avatar_url: Option<String>,
    pub role: Option<String>,
    pub responsibility: Option<String>,
    pub bound_model: Option<String>,
    pub provider: Option<String>,
    /// 绑定来源："provider" | "cli" | null。配合 provider/cli_agent_* 字段使用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_kind: Option<String>,
    /// CLI Agent 名称（binding_kind="cli" 时填充）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_agent_name: Option<String>,
    /// CLI Agent 方言："claude_code" | "codex" | "plain"。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_agent_flavor: Option<String>,
    /// CLI Agent 模型标识（来自 AgentProfile.model_id）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_agent_model: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ConversationUpdateInput {
    pub title: Option<String>,
    pub goal: Option<String>,
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
    let session_id = state.kernel.ensure_session_id().await?;
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
            if let Some(ws) = repos::workspace_open_state::find_focused(&db.0)? {
                repos::tasks_runs::bind_run_workspace(&db.0, &run.id, &ws.workspace_id)?;
            }
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

/// Like [`transition_run_with_detail`] but also publishes `run.state_changed`
/// to the kernel bus when `bus` is provided (plan §5.3).
pub(crate) fn transition_run_and_notify(
    conn: &rusqlite::Connection,
    bus: Option<&nuomi_core::harness::EventBus>,
    run_id: &str,
    expected: RunState,
    ev: RunEvent,
    error_message: Option<&str>,
) -> Result<RunState, IpcError> {
    let next = transition_run_with_detail(conn, run_id, expected, ev, error_message)?;
    if let Some(bus) = bus {
        let mut payload = serde_json::json!({
            "runId": run_id,
            "from": expected.as_str(),
            "to": next.as_str(),
        });
        if let Some(msg) = error_message {
            payload["error"] = serde_json::Value::String(msg.to_string());
        }
        bus.publish(nuomi_core::harness::Event::new("run.state_changed", payload));
    }
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

pub async fn impl_git_diff(
    state: &AppState,
    path: String,
    staged: bool,
) -> Result<String, IpcError> {
    let out = state.git().diff_for_path(&path, staged).await?;
    if out.trim().is_empty() {
        // Untracked files have no diff at all (they only ever appear in the
        // status list), so synthesize one against /dev/null. A tracked file
        // with an empty diff is not in the status list in the first place.
        return Ok(state.git().diff_untracked(&path).await?);
    }
    Ok(out)
}

pub async fn impl_git_staged_diff(state: &AppState) -> Result<String, IpcError> {
    Ok(state.git().diff_staged().await?)
}

// ---------- ai commit ----------

pub async fn impl_list_commit_agents(
    state: &AppState,
) -> Result<Vec<CommitAgentOptionDto>, IpcError> {
    let path = state.db_path.clone();
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<_>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(nuomi_core::services::ai_commit_list_commit_agents(&db.0)?)
    })
    .await??;
    Ok(rows.into_iter().map(CommitAgentOptionDto::from).collect())
}

pub async fn impl_ai_commit_generate(
    state: &AppState,
    role_agent: Option<AgentRefInput>,
) -> Result<AiCommitResultDto, IpcError> {
    let parsed = role_agent
        .as_ref()
        .and_then(|a| AgentRefKind::parse(&a.kind).map(|k| (k, a.id.clone())));

    let git = state.git();
    let db_path = state.db_path.clone();
    let secrets = state.secrets.clone();
    let cwd = Some(state.current_workspace());

    match nuomi_core::services::ai_commit_generate(parsed, db_path, secrets, cwd, &git).await {
        Ok(result) => Ok(AiCommitResultDto::from(result)),
        Err(e) => Err(map_ai_commit_error(e)),
    }
}

fn map_ai_commit_error(e: nuomi_core::services::AiCommitError) -> IpcError {
    use nuomi_core::services::AiCommitError;
    match e {
        AiCommitError::NoStagedChanges => {
            IpcError::new("ai_commit.no_staged_changes", "no staged changes to commit")
        }
        AiCommitError::AgentUnavailable(msg) => {
            IpcError::new("ai_commit.agent_unavailable", msg)
        }
        AiCommitError::GenerationFailed(msg) => {
            IpcError::new("ai_commit.generation_failed", msg)
        }
        AiCommitError::EmptyResult => {
            IpcError::new("ai_commit.empty_result", "AI returned empty content")
        }
        AiCommitError::Timeout(ms) => {
            IpcError::new("ai_commit.timeout", format!("generation timed out after {ms}ms"))
        }
        AiCommitError::Join(e) => IpcError::from(e),
        AiCommitError::Core(e) => IpcError::from(e),
    }
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
        target_kind: ScheduleTargetKind::Task,
        agent: None,
        team_id: None,
        session_mode: ScheduleSessionMode::PerTrigger,
        session_id: None,
        auto_dispatch: true,
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
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<ScheduleDto>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let schedules = repos::tasks_runs::list_schedules(&db.0, 200)?;
        let mut out = Vec::with_capacity(schedules.len());
        for schedule in &schedules {
            let mut dto = ScheduleDto::from(schedule.clone());
            // Same display-name fill as the conversation list: a row should
            // read "Codex", not "cli:<id>".
            if let Some(agent) =
                nuomi_core::services::name_agent_ref(&db.0, schedule.agent.as_ref())?
            {
                dto.agent = Some(AgentRefDto {
                    kind: agent.kind.as_str().to_string(),
                    id: agent.id,
                    name: agent.name,
                });
            }
            out.push(dto);
        }
        Ok(out)
    })
    .await??;
    Ok(rows)
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

/// Provider-level settings mirrored from
/// `nuomi_core::domain::entities::ProviderSettings` (stored inside the
/// provider row's `params_json` under the `"settings"` key).
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSettingsDto {
    /// Per-model entries with capability tags. Legacy string model ids are
    /// normalized to `{ id, capabilities: ["reasoning"] }` by the entity.
    #[serde(default)]
    pub models: Vec<ModelEntryDto>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    #[serde(default)]
    pub timeout_secs: Option<i64>,
    #[serde(default)]
    pub retry: Option<i64>,
    #[serde(default)]
    pub max_concurrency: Option<i64>,
    #[serde(default)]
    pub priority: Option<f64>,
    #[serde(default)]
    pub roles: Vec<String>,
    /// Per-provider local network proxy (`http://host:port`); `None` = direct.
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default)]
    pub enabled: bool,
}

/// One model exposed by a provider endpoint plus its capability tags
/// (KiloCode-style per-model capabilities).
#[derive(Debug, Clone, PartialEq, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntryDto {
    pub id: String,
    pub capabilities: Vec<CapabilityDto>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
}

/// System modality capability (mirrors `nuomi_core::domain::Capability`).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    serde::Deserialize,
    specta::Type,
)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityDto {
    Reasoning,
    Image,
    Voice,
    Video,
}

impl From<nuomi_core::domain::Capability> for CapabilityDto {
    fn from(c: nuomi_core::domain::Capability) -> Self {
        match c {
            nuomi_core::domain::Capability::Reasoning => Self::Reasoning,
            nuomi_core::domain::Capability::Image => Self::Image,
            nuomi_core::domain::Capability::Voice => Self::Voice,
            nuomi_core::domain::Capability::Video => Self::Video,
        }
    }
}

impl From<CapabilityDto> for nuomi_core::domain::Capability {
    fn from(c: CapabilityDto) -> Self {
        match c {
            CapabilityDto::Reasoning => nuomi_core::domain::Capability::Reasoning,
            CapabilityDto::Image => nuomi_core::domain::Capability::Image,
            CapabilityDto::Voice => nuomi_core::domain::Capability::Voice,
            CapabilityDto::Video => nuomi_core::domain::Capability::Video,
        }
    }
}

fn caps_to_dto(caps: &[nuomi_core::domain::Capability]) -> Vec<CapabilityDto> {
    caps.iter().copied().map(CapabilityDto::from).collect()
}

fn caps_from_dto(caps: &[CapabilityDto]) -> Vec<nuomi_core::domain::Capability> {
    caps.iter()
        .copied()
        .map(nuomi_core::domain::Capability::from)
        .collect()
}

impl ProviderSettingsDto {
    fn from_entity(settings: nuomi_core::domain::entities::ProviderSettings) -> Self {
        Self {
            models: settings
                .models
                .into_iter()
                .map(|m| ModelEntryDto {
                    id: m.id,
                    capabilities: caps_to_dto(&m.capabilities),
                    temperature: m.temperature,
                    top_p: m.top_p,
                    max_tokens: m.max_tokens,
                })
                .collect(),
            default_model: settings.default_model,
            temperature: settings.temperature,
            top_p: settings.top_p,
            max_tokens: settings.max_tokens,
            timeout_secs: settings.timeout_secs,
            retry: settings.retry,
            max_concurrency: settings.max_concurrency,
            priority: settings.priority,
            roles: settings.roles,
            proxy: settings.proxy,
            enabled: settings.enabled,
        }
    }

    fn into_entity(self) -> nuomi_core::domain::entities::ProviderSettings {
        nuomi_core::domain::entities::ProviderSettings {
            models: self
                .models
                .into_iter()
                .map(|m| nuomi_core::domain::ModelEntry {
                    id: m.id,
                    capabilities: caps_from_dto(&m.capabilities),
                    temperature: m.temperature,
                    top_p: m.top_p,
                    max_tokens: m.max_tokens,
                })
                .collect(),
            default_model: self.default_model,
            temperature: self.temperature,
            top_p: self.top_p,
            max_tokens: self.max_tokens,
            timeout_secs: self.timeout_secs,
            retry: self.retry,
            max_concurrency: self.max_concurrency,
            priority: self.priority,
            roles: self.roles,
            proxy: self.proxy,
            enabled: self.enabled,
        }
    }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    /// `None` inserts a fresh row; `Some(id)` updates that row in place.
    pub id: Option<String>,
    pub name: String,
    pub protocol: ProviderProtocolDto,
    pub base_url: String,
    pub capabilities: Vec<String>,
    pub is_master: bool,
    /// Plaintext only in transit — stored straight into the OS keyring,
    /// never persisted to SQLite or logs.
    pub api_key: Option<String>,
    /// Model/routing settings (persisted inside `params_json`).
    pub settings: ProviderSettingsDto,
}

fn protocol_from_dto(p: ProviderProtocolDto) -> ProviderProtocol {
    match p {
        ProviderProtocolDto::OpenAiCompatible => ProviderProtocol::OpenAiCompatible,
        ProviderProtocolDto::AnthropicCompatible => ProviderProtocol::AnthropicCompatible,
    }
}

fn protocol_to_dto(p: ProviderProtocol) -> ProviderProtocolDto {
    match p {
        ProviderProtocol::OpenAiCompatible => ProviderProtocolDto::OpenAiCompatible,
        ProviderProtocol::AnthropicCompatible => ProviderProtocolDto::AnthropicCompatible,
    }
}

/// `id` is the update handle: an existing provider id is updated in place
/// (keeping `created_at` and — when no new key is supplied — the stored
/// keyring reference), otherwise a fresh row is inserted. The `name` column
/// stays UNIQUE, so renaming onto a taken name fails at the store layer.
pub async fn impl_upsert_provider(
    state: &AppState,
    provider: ProviderInput,
) -> Result<(), IpcError> {
    use nuomi_core::providers::SecretStore;
    if let Some(key) = provider.api_key.as_ref() {
        let reference = format!("provider/{}", provider.name);
        let store = nuomi_core::providers::OsKeyring;
        store.set(&reference, key).await?;
    }
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let now = now_ms();
        let existing = match provider.id.as_deref() {
            Some(id) => match repos::providers::get_provider(&db.0, id) {
                Ok(row) => Some(row),
                Err(StoreError::NotFound { .. }) => {
                    return Err(IpcError::new(
                        "provider.not_found",
                        format!("provider#{id} not found"),
                    ))
                }
                Err(e) => return Err(e.into()),
            },
            None => None,
        };
        let keyring_ref = match &provider.api_key {
            Some(_) => Some(format!("provider/{}", provider.name)),
            None => existing.as_ref().and_then(|row| row.keyring_ref.clone()),
        };
        let base_params = existing
            .as_ref()
            .map(|row| row.params.clone())
            .unwrap_or_else(|| serde_json::json!({}));
        let params = provider.settings.into_entity().into_params(base_params);
        let config = ProviderConfig {
            id: existing
                .as_ref()
                .map(|row| row.id.clone())
                .unwrap_or_else(nuomi_core::domain::new_id),
            name: provider.name,
            protocol: protocol_from_dto(provider.protocol),
            base_url: provider.base_url,
            keyring_ref,
            capabilities: provider.capabilities,
            is_master: provider.is_master,
            fallback_order: existing.as_ref().and_then(|row| row.fallback_order),
            params,
            created_at: existing.as_ref().map(|row| row.created_at).unwrap_or(now),
            updated_at: now,
        };
        if existing.is_some() {
            Ok(repos::providers::update_provider(&db.0, &config)?)
        } else {
            Ok(repos::providers::insert_provider(&db.0, &config)?)
        }
    })
    .await?
}

pub async fn impl_delete_provider(
    state: &AppState,
    provider_id: String,
    force: bool,
) -> Result<(), IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let mut db = Db::open(&path)?;
        migrations::run(&db.0)?;

        // Verify the provider exists.
        if repos::providers::get_provider(&db.0, &provider_id).is_err() {
            return Err(IpcError::new(
                "provider.not_found",
                format!("provider#{provider_id} not found"),
            ));
        }

        // Two-phase delete (ADR 0011 D5): phase one — check refs.
        let refs = check_provider_refs(&db.0, &provider_id)?;
        if !refs.is_empty() && !force {
            return Err(IpcError::with_details(
                "entity.referenced",
                format!(
                    "provider#{provider_id} is referenced by {} role(s)",
                    refs.roles.len()
                ),
                serde_json::to_value(&refs).unwrap_or_default(),
            ));
        }

        // Phase two (force=true or no refs): delete + nullify in one tx.
        delete_and_nullify_provider_refs(&mut db.0, &provider_id)?;
        Ok(())
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
            protocol: protocol_to_dto(p.protocol),
            base_url: p.base_url,
            has_key: p.keyring_ref.is_some(),
            capabilities: p.capabilities,
            is_master: p.is_master,
            settings: ProviderSettingsDto::from_entity(
                nuomi_core::domain::entities::ProviderSettings::from_params(&p.params),
            ),
        })
        .collect())
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TestProviderConnectionInput {
    /// When set and `api_key` is empty, the stored keyring secret is used.
    pub provider_id: Option<String>,
    pub protocol: ProviderProtocolDto,
    pub base_url: String,
    pub api_key: Option<String>,
    /// Overrides the stored per-provider proxy when non-empty.
    pub proxy: Option<String>,
    /// Model for the minimal chat probe; protocol defaults apply when empty.
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TestProviderConnectionDto {
    pub ok: bool,
    pub latency_ms: Option<i64>,
    pub error: Option<String>,
}

/// API key + proxy for a read-only probe: explicitly typed values win,
/// otherwise both come from the stored provider row (`provider_id`); missing
/// rows/keys degrade to empty (direct connection). One DB read for both.
async fn resolve_probe_secrets(
    state: &AppState,
    provider_id: Option<&str>,
    api_key: Option<String>,
    proxy: Option<String>,
) -> Result<(String, Option<String>), IpcError> {
    use nuomi_core::providers::SecretStore;
    let typed_key = api_key.filter(|k| !k.is_empty());
    let typed_proxy = proxy
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    if typed_key.is_some() && typed_proxy.is_some() {
        return Ok((typed_key.unwrap_or_default(), typed_proxy));
    }
    let Some(provider_id) = provider_id else {
        return Ok((typed_key.unwrap_or_default(), typed_proxy));
    };
    let path = state.db_path.clone();
    let pid = provider_id.to_string();
    let (stored_key, stored_proxy) = tokio::task::spawn_blocking(
        move || -> Result<(Option<String>, Option<String>), IpcError> {
            let db = Db::open(&path)?;
            migrations::run(&db.0)?;
            match repos::providers::get_provider(&db.0, &pid) {
                Ok(row) => {
                    let settings =
                        nuomi_core::domain::entities::ProviderSettings::from_params(&row.params);
                    Ok((row.keyring_ref, settings.proxy))
                }
                Err(StoreError::NotFound { .. }) => Err(IpcError::new(
                    "provider.not_found",
                    format!("provider#{pid} not found"),
                )),
                Err(e) => Err(e.into()),
            }
        },
    )
    .await??;
    // Keep the original semantics: typed key wins, else keyring secret.
    let api_key = match typed_key {
        Some(k) => k,
        None => match stored_key {
            Some(reference) => nuomi_core::providers::OsKeyring
                .get(&reference)
                .await
                .unwrap_or_default(),
            None => String::new(),
        },
    };
    Ok((api_key, typed_proxy.or(stored_proxy)))
}

/// Read-only connectivity probe (5s budget): resolves the API key (explicit
/// input, else the stored keyring secret for `provider_id`), then sends a
/// one-token chat through the matching core client. Never persists anything;
/// failures come back as `ok:false`, never as IPC errors.
pub async fn impl_test_provider_connection(
    state: &AppState,
    input: TestProviderConnectionInput,
) -> Result<TestProviderConnectionDto, IpcError> {
    use nuomi_core::providers::LlmProvider;
    /// Probe budget; mirrors the CLI-agent check so a wedged endpoint
    /// surfaces as `ok:false`, never a hang.
    const TEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    let (api_key, proxy) = resolve_probe_secrets(
        state,
        input.provider_id.as_deref(),
        input.api_key,
        input.proxy,
    )
    .await?;

    let model = input
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| match input.protocol {
            ProviderProtocolDto::OpenAiCompatible => "gpt-4o-mini".to_string(),
            ProviderProtocolDto::AnthropicCompatible => "claude-3-5-haiku-latest".to_string(),
        });
    let mut request =
        nuomi_core::providers::ChatRequest::simple(&model, "connection probe", "ping");
    request.temperature = Some(0.0);
    request.max_tokens = Some(1);

    let started = std::time::Instant::now();
    // Per-provider proxy: build the client before starting the latency clock
    // (a builder failure is a config error, not probe latency).
    let http = nuomi_core::providers::pool::client_for_endpoint(proxy.as_deref())?;
    let probe: std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        nuomi_core::providers::ChatResponse,
                        nuomi_core::providers::ProviderError,
                    >,
                > + Send,
        >,
    > = match input.protocol {
        ProviderProtocolDto::OpenAiCompatible => Box::pin(async {
            nuomi_core::providers::OpenAiCompatibleClient::new(&input.base_url, &api_key)
                .with_http_client(http)
                .complete(&request)
                .await
        }),
        ProviderProtocolDto::AnthropicCompatible => Box::pin(async {
            nuomi_core::providers::AnthropicCompatibleClient::new(&input.base_url, &api_key)
                .with_http_client(http)
                .complete(&request)
                .await
        }),
    };
    let result = tokio::time::timeout(TEST_TIMEOUT, probe).await;
    let latency_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    Ok(match result {
        Ok(Ok(_)) => TestProviderConnectionDto {
            ok: true,
            latency_ms: Some(latency_ms),
            error: None,
        },
        Ok(Err(e)) => TestProviderConnectionDto {
            ok: false,
            latency_ms: Some(latency_ms),
            error: Some(e.to_string()),
        },
        Err(_elapsed) => TestProviderConnectionDto {
            ok: false,
            latency_ms: Some(latency_ms),
            error: Some(format!("timed out after {}s", TEST_TIMEOUT.as_secs())),
        },
    })
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ListProviderModelsInput {
    /// When set and `api_key` is empty, the stored keyring secret is used.
    pub provider_id: Option<String>,
    pub protocol: ProviderProtocolDto,
    pub base_url: String,
    pub api_key: Option<String>,
    /// Overrides the stored per-provider proxy when non-empty.
    pub proxy: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ListProviderModelsDto {
    /// Sorted, de-duplicated ids exposed by `GET {base_url}/models`.
    pub models: Vec<String>,
    /// Set instead of an IPC error on transport/HTTP/parse failures, so the
    /// settings form keeps the previously fetched list on screen.
    pub error: Option<String>,
}

/// Read-only model catalog fetch (KiloCode-style picker): resolves the API
/// key exactly like the connectivity probe, then lists the ids the endpoint
/// exposes. Never persists anything.
pub async fn impl_list_provider_models(
    state: &AppState,
    input: ListProviderModelsInput,
) -> Result<ListProviderModelsDto, IpcError> {
    let (api_key, proxy) = resolve_probe_secrets(
        state,
        input.provider_id.as_deref(),
        input.api_key,
        input.proxy,
    )
    .await?;
    let protocol = protocol_from_dto(input.protocol);
    match nuomi_core::providers::list_model_ids(
        protocol,
        &input.base_url,
        &api_key,
        proxy.as_deref(),
    )
    .await
    {
        Ok(models) => Ok(ListProviderModelsDto {
            models,
            error: None,
        }),
        Err(e) => Ok(ListProviderModelsDto {
            models: Vec::new(),
            error: Some(e.to_string()),
        }),
    }
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

// ---------- evolution settings (自进化四维度) ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RefineStrategyDto {
    PromptNote,
    Memory,
    Skill,
    SubAgentSpec,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SkillFormatDto {
    SkillMd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalStrategyDto {
    Keyword,
    Semantic,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OnlineLearningConfigDto {
    pub authorized: bool,
    pub allowlist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RefineConfigDto {
    pub trigger_failures: u32,
    pub min_edit_strategy: RefineStrategyDto,
    pub evidence_threshold: f64,
    pub rollback_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillCreationConfigDto {
    pub enabled: bool,
    pub format: SkillFormatDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MemoryPolicyDto {
    pub retention_days: u32,
    pub retrieval: RetrievalStrategyDto,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EvolutionSettingsDto {
    pub online_learning: OnlineLearningConfigDto,
    pub refine: RefineConfigDto,
    pub skill_creation: SkillCreationConfigDto,
    pub memory_policy: MemoryPolicyDto,
}

const EVOLUTION_SETTINGS_KEY: &str = "evolution_settings";

impl EvolutionSettingsDto {
    fn from_entity(e: nuomi_core::domain::EvolutionSettings) -> Self {
        use nuomi_core::domain::{
            MemoryPolicy, OnlineLearningConfig, RefineConfig, SkillCreationConfig,
        };
        let map_refine = |s: nuomi_core::domain::RefineStrategy| match s {
            nuomi_core::domain::RefineStrategy::PromptNote => RefineStrategyDto::PromptNote,
            nuomi_core::domain::RefineStrategy::Memory => RefineStrategyDto::Memory,
            nuomi_core::domain::RefineStrategy::Skill => RefineStrategyDto::Skill,
            nuomi_core::domain::RefineStrategy::SubAgentSpec => RefineStrategyDto::SubAgentSpec,
        };
        let map_skill = |s: nuomi_core::domain::SkillFormat| match s {
            nuomi_core::domain::SkillFormat::SkillMd => SkillFormatDto::SkillMd,
        };
        let map_retrieval = |s: nuomi_core::domain::RetrievalStrategy| match s {
            nuomi_core::domain::RetrievalStrategy::Keyword => RetrievalStrategyDto::Keyword,
            nuomi_core::domain::RetrievalStrategy::Semantic => RetrievalStrategyDto::Semantic,
            nuomi_core::domain::RetrievalStrategy::Hybrid => RetrievalStrategyDto::Hybrid,
        };
        let OnlineLearningConfig { authorized, allowlist } = e.online_learning;
        let RefineConfig {
            trigger_failures,
            min_edit_strategy,
            evidence_threshold,
            rollback_enabled,
        } = e.refine;
        let SkillCreationConfig { enabled, format } = e.skill_creation;
        let MemoryPolicy { retention_days, retrieval } = e.memory_policy;
        Self {
            online_learning: OnlineLearningConfigDto { authorized, allowlist },
            refine: RefineConfigDto {
                trigger_failures,
                min_edit_strategy: map_refine(min_edit_strategy),
                evidence_threshold,
                rollback_enabled,
            },
            skill_creation: SkillCreationConfigDto {
                enabled,
                format: map_skill(format),
            },
            memory_policy: MemoryPolicyDto {
                retention_days,
                retrieval: map_retrieval(retrieval),
            },
        }
    }

    fn to_entity(&self) -> nuomi_core::domain::EvolutionSettings {
        use nuomi_core::domain::{
            EvolutionSettings, MemoryPolicy, OnlineLearningConfig, RefineConfig,
            SkillCreationConfig,
        };
        let map_refine = |s: RefineStrategyDto| match s {
            RefineStrategyDto::PromptNote => nuomi_core::domain::RefineStrategy::PromptNote,
            RefineStrategyDto::Memory => nuomi_core::domain::RefineStrategy::Memory,
            RefineStrategyDto::Skill => nuomi_core::domain::RefineStrategy::Skill,
            RefineStrategyDto::SubAgentSpec => nuomi_core::domain::RefineStrategy::SubAgentSpec,
        };
        let map_skill = |s: SkillFormatDto| match s {
            SkillFormatDto::SkillMd => nuomi_core::domain::SkillFormat::SkillMd,
        };
        let map_retrieval = |s: RetrievalStrategyDto| match s {
            RetrievalStrategyDto::Keyword => nuomi_core::domain::RetrievalStrategy::Keyword,
            RetrievalStrategyDto::Semantic => nuomi_core::domain::RetrievalStrategy::Semantic,
            RetrievalStrategyDto::Hybrid => nuomi_core::domain::RetrievalStrategy::Hybrid,
        };
        EvolutionSettings {
            online_learning: OnlineLearningConfig {
                authorized: self.online_learning.authorized,
                allowlist: self.online_learning.allowlist.clone(),
            },
            refine: RefineConfig {
                trigger_failures: self.refine.trigger_failures,
                min_edit_strategy: map_refine(self.refine.min_edit_strategy),
                evidence_threshold: self.refine.evidence_threshold,
                rollback_enabled: self.refine.rollback_enabled,
            },
            skill_creation: SkillCreationConfig {
                enabled: self.skill_creation.enabled,
                format: map_skill(self.skill_creation.format),
            },
            memory_policy: MemoryPolicy {
                retention_days: self.memory_policy.retention_days,
                retrieval: map_retrieval(self.memory_policy.retrieval),
            },
        }
    }
}

pub async fn impl_get_evolution_settings(
    state: &AppState,
) -> Result<EvolutionSettingsDto, IpcError> {
    let path = state.db_path.clone();
    let json = tokio::task::spawn_blocking(move || -> Result<Option<String>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        repos::settings::get(&db.0, EVOLUTION_SETTINGS_KEY)
            .map_err(|e| IpcError::new("settings.get_failed", e.to_string()))
    })
    .await
    .map_err(|e| IpcError::new("settings.get_failed", e.to_string()))??;

    if let Some(raw) = json {
        let mut settings: nuomi_core::domain::EvolutionSettings =
            serde_json::from_str(&raw)
                .map_err(|e| IpcError::new("evolution.parse_failed", e.to_string()))?;
        let mem = nuomi_core::plugins::MemoryService::new(state.db_path.clone());
        let legacy_auth = core_online_authorized(&mem).await;
        if !settings.online_learning.authorized && legacy_auth {
            settings.online_learning.authorized = true;
        }
        return Ok(EvolutionSettingsDto::from_entity(settings));
    }

    let mem = nuomi_core::plugins::MemoryService::new(state.db_path.clone());
    let legacy_auth = core_online_authorized(&mem).await;
    let mut defaults = nuomi_core::domain::EvolutionSettings::default();
    defaults.online_learning.authorized = legacy_auth;
    Ok(EvolutionSettingsDto::from_entity(defaults))
}

pub async fn impl_set_evolution_settings(
    state: &AppState,
    dto: EvolutionSettingsDto,
) -> Result<(), IpcError> {
    if dto.refine.evidence_threshold < 0.5 {
        return Err(IpcError::new(
            "evolution.invalid_config",
            "evidence_threshold must be >= 0.5",
        ));
    }
    if dto.refine.trigger_failures < 1 {
        return Err(IpcError::new(
            "evolution.invalid_config",
            "trigger_failures must be >= 1",
        ));
    }
    if dto.memory_policy.retention_days < 1 {
        return Err(IpcError::new(
            "evolution.invalid_config",
            "retention_days must be >= 1",
        ));
    }

    let entity = dto.to_entity();
    let raw = serde_json::to_string(&entity)
        .map_err(|e| IpcError::new("evolution.serialize_failed", e.to_string()))?;

    let path = state.db_path.clone();
    let raw_clone = raw.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        repos::settings::set(&db.0, EVOLUTION_SETTINGS_KEY, &raw_clone)
            .map_err(|e| IpcError::new("settings.set_failed", e.to_string()))
    })
    .await
    .map_err(|e| IpcError::new("settings.set_failed", e.to_string()))??;

    let mem = nuomi_core::plugins::MemoryService::new(state.db_path.clone());
    set_online_authorized(&mem, entity.online_learning.authorized).await?;
    Ok(())
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
    pub model_id: Option<String>,
    /// CLI 会话保持参数模板（ADR 0012 D6）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_args: Option<String>,
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
            model_id: p.model_id,
            resume_args: p.resume_args,
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
    #[serde(default)]
    pub model_id: Option<String>,
    /// CLI 会话保持参数模板（ADR 0012 D6）。如 `--resume {session_id}`。
    #[serde(default)]
    pub resume_args: Option<String>,
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
            model_id: self.model_id,
            resume_args: self.resume_args,
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
                prev.model_id = profile.model_id;
                prev.resume_args = profile.resume_args;
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

    // On Windows a bare name like `codebuddy` may resolve to an extensionless
    // `#!/bin/sh` shim that `CreateProcess` cannot execute; retry PATHEXT
    // variants (`.cmd`/`.bat`/`.exe`) before giving up.
    let candidates = nuomi_core::adapters::spawn_candidates(&profile.command);
    let mut child = None;
    let mut last_err = None;
    for candidate in &candidates {
        let mut cmd = tokio::process::Command::new(candidate);
        cmd.arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        match cmd.spawn() {
            Ok(c) => {
                child = Some(c);
                break;
            }
            Err(e) => last_err = Some(e),
        }
    }
    let mut child = match child {
        Some(c) => c,
        None => {
            let err = last_err.unwrap_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "no spawn candidates")
            });
            return Ok(failed_check(format!(
                "failed to spawn {}: {err}",
                profile.command
            )));
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
    pub provider_ids: Vec<String>,
    pub system_prompt_override: Option<String>,
    pub tool_allowlist: Vec<String>,
    pub required_capabilities: Vec<CapabilityDto>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i64>,
    pub params: serde_json::Value,
    pub builtin: bool,
    pub generated: bool,
    pub ephemeral: bool,
    pub source: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
}

impl From<Role> for RoleDto {
    fn from(r: Role) -> Self {
        Self {
            id: r.id,
            name: r.name,
            provider_id: r.provider_id,
            provider_ids: r.provider_ids,
            system_prompt_override: r.system_prompt_override,
            tool_allowlist: r.tool_allowlist,
            required_capabilities: caps_to_dto(&r.required_capabilities),
            temperature: r.temperature,
            max_tokens: r.max_tokens,
            params: r.params,
            builtin: r.builtin,
            generated: r.generated,
            ephemeral: r.ephemeral,
            source: r.source.unwrap_or(serde_json::Value::Null),
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
    /// Multi-provider bindings (Agent = Role + Provider); merged with
    /// `provider_id` when both are supplied.
    #[serde(default)]
    pub provider_ids: Vec<String>,
    pub system_prompt_override: Option<String>,
    pub tool_allowlist: Vec<String>,
    #[serde(default)]
    pub required_capabilities: Vec<CapabilityDto>,
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

/// Validates that every bound provider's capability union covers the role's
/// `required_capabilities` (`role.capability_mismatch` with the missing set
/// otherwise).
fn validate_role_provider_capabilities(
    conn: &rusqlite::Connection,
    role_name: &str,
    provider_ids: &[String],
    required: &[nuomi_core::domain::Capability],
) -> Result<(), IpcError> {
    for pid in provider_ids {
        let provider = match repos::providers::get_provider(conn, pid) {
            Ok(p) => p,
            Err(StoreError::NotFound { .. }) => {
                return Err(IpcError::new(
                    "provider.not_found",
                    format!("provider#{pid} not found"),
                ))
            }
            Err(e) => return Err(e.into()),
        };
        let missing: Vec<String> = required
            .iter()
            .filter(|c| !provider.capability_union().contains(c))
            .map(|c| c.as_str().to_string())
            .collect();
        if !missing.is_empty() {
            return Err(IpcError::with_details(
                "role.capability_mismatch",
                format!(
                    "provider '{}' does not cover the capabilities required by role '{}': {}",
                    provider.name,
                    role_name,
                    missing.join(", ")
                ),
                serde_json::json!({
                    "providerId": pid,
                    "missingCapabilities": missing,
                }),
            ));
        }
    }
    Ok(())
}

/// `name` is the idempotency key: an existing role with the same name is
/// updated in place (keeping its `id`/`created_at`), otherwise inserted.
/// Builtin flags (`builtin`/`generated`/`ephemeral`) are system-owned and
/// never settable from the input.
pub async fn impl_upsert_role(state: &AppState, role: RoleInput) -> Result<RoleDto, IpcError> {
    role.validate()?;
    let path = state.db_path.clone();
    let entity = tokio::task::spawn_blocking(move || -> Result<Role, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let now = now_ms();
        let mut provider_ids = role.provider_ids;
        if let Some(pid) = role.provider_id.as_ref() {
            if !provider_ids.contains(pid) {
                provider_ids.insert(0, pid.clone());
            }
        }
        let required = caps_from_dto(&role.required_capabilities);
        validate_role_provider_capabilities(&db.0, role.name.trim(), &provider_ids, &required)?;
        let existing = repos::roles::list(&db.0)?
            .into_iter()
            .find(|r| r.name == role.name);
        match existing {
            Some(mut prev) => {
                prev.provider_id = provider_ids.first().cloned();
                prev.provider_ids = provider_ids;
                prev.system_prompt_override = role.system_prompt_override;
                prev.tool_allowlist = role.tool_allowlist;
                prev.required_capabilities = required;
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
                    provider_id: provider_ids.first().cloned(),
                    provider_ids,
                    system_prompt_override: role.system_prompt_override,
                    tool_allowlist: role.tool_allowlist,
                    required_capabilities: required,
                    temperature: role.temperature,
                    max_tokens: role.max_tokens,
                    params: role.params,
                    builtin: false,
                    generated: false,
                    ephemeral: false,
                    source: None,
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

pub async fn impl_delete_role(
    state: &AppState,
    role_id: String,
    force: bool,
) -> Result<(), IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let mut db = Db::open(&path)?;
        migrations::run(&db.0)?;
        // Built-in preset roles are protected from deletion (disable/rebind
        // instead); everything else deletes normally.
        if let Ok(role) = repos::roles::get(&db.0, &role_id) {
            if role.builtin {
                return Err(IpcError::new(
                    "role.builtin_protected",
                    format!("role#{role_id} is a built-in preset and cannot be deleted"),
                ));
            }
        } else {
            return Err(IpcError::new(
                "role.not_found",
                format!("role#{role_id} not found"),
            ));
        }

        // Two-phase delete (ADR 0011 D5): phase one — check refs.
        let refs = check_role_refs(&db.0, &role_id)?;
        if !refs.is_empty() && !force {
            return Err(IpcError::with_details(
                "entity.referenced",
                format!(
                    "role#{role_id} is referenced by {} team(s) and {} session(s)",
                    refs.teams.len(),
                    refs.sessions.len()
                ),
                serde_json::to_value(&refs).unwrap_or_default(),
            ));
        }

        // Phase two (force=true or no refs): delete + nullify in one tx.
        delete_and_nullify_role_refs(&mut db.0, &role_id)?;
        Ok(())
    })
    .await?
}

// ---------- role capability system (presets / director / routing) ----------

/// Outcome counts of a preset seeding pass.
#[derive(Debug, Clone, Copy, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SeedRolesDto {
    pub inserted: u64,
    pub updated: u64,
    pub skipped: u64,
}

/// Idempotently seeds the built-in preset roles ("restore presets").
pub async fn impl_seed_builtin_roles(state: &AppState) -> Result<SeedRolesDto, IpcError> {
    let path = state.db_path.clone();
    let report = tokio::task::spawn_blocking(move || -> Result<SeedReport, IpcError> {
        let mut db = Db::open(&path)?;
        Ok(nuomi_core::services::presets::seed_builtin_roles(
            &mut db.0,
        )?)
    })
    .await??;
    Ok(SeedRolesDto {
        inserted: report.inserted as u64,
        updated: report.updated as u64,
        skipped: report.skipped as u64,
    })
}

/// Role Director: LLM-generates a structured role from a plain-language
/// description, validates it and persists it (`generated = true`).
pub async fn impl_generate_role(
    state: &AppState,
    description: String,
) -> Result<RoleDto, IpcError> {
    if description.trim().is_empty() {
        return Err(IpcError::new(
            "role.director_invalid",
            "description must not be empty",
        ));
    }
    let role = nuomi_core::services::role_director::generate_role(
        state.db_path.clone(),
        state.secrets.clone(),
        Some(state.current_workspace()),
        &description,
    )
    .await
    .map_err(|e| match e {
        nuomi_core::services::role_director::RoleDirectorError::NoProvider => IpcError::new(
            "role.no_provider",
            "no provider available to generate a role",
        ),
        nuomi_core::services::role_director::RoleDirectorError::Rejected(msg) => {
            IpcError::new("role.director_invalid", msg)
        }
        other => IpcError::new("role.director_failed", other.to_string()),
    })?;
    Ok(RoleDto::from(role))
}

/// Routing rules mirrored from `nuomi_core::services::RoutingRules`
/// (persisted in `app_settings`).
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RoutingRulesDto {
    #[serde(default)]
    pub prefer_local: bool,
    #[serde(default)]
    pub capability_overrides: BTreeMap<CapabilityDto, String>,
}

impl RoutingRulesDto {
    fn into_entity(self) -> nuomi_core::services::RoutingRules {
        nuomi_core::services::RoutingRules {
            prefer_local: self.prefer_local,
            capability_overrides: self
                .capability_overrides
                .into_iter()
                .map(|(k, v)| (k.into(), v))
                .collect(),
        }
    }

    fn from_entity(rules: nuomi_core::services::RoutingRules) -> Self {
        Self {
            prefer_local: rules.prefer_local,
            capability_overrides: rules
                .capability_overrides
                .into_iter()
                .map(|(k, v)| (CapabilityDto::from(k), v))
                .collect(),
        }
    }
}

pub async fn impl_get_routing_rules(state: &AppState) -> Result<RoutingRulesDto, IpcError> {
    let path = state.db_path.clone();
    let rules = tokio::task::spawn_blocking(move || -> Result<RoutingRulesDto, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        nuomi_core::services::capability_router::load_routing_rules(&db.0)
            .map(RoutingRulesDto::from_entity)
            .map_err(|e| IpcError::new("route.failed", e.to_string()))
    })
    .await??;
    Ok(rules)
}

pub async fn impl_set_routing_rules(
    state: &AppState,
    rules: RoutingRulesDto,
) -> Result<(), IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        nuomi_core::services::capability_router::save_routing_rules(&db.0, &rules.into_entity())
            .map_err(|e| IpcError::new("route.failed", e.to_string()))
    })
    .await?
}

/// A capability-routing probe: resolves a role for the required capability
/// set (optionally creating + immediately cleaning an ephemeral temp role
/// when `dryRun` is false and only a provider can serve).
#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RouteRequestDto {
    pub required_capabilities: Vec<CapabilityDto>,
    pub prefer_role_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RouteResultDto {
    pub role: RoleDto,
    /// True when the router created an ephemeral temp role for the request.
    pub created_temp: bool,
}

pub async fn impl_route_capability(
    state: &AppState,
    request: RouteRequestDto,
) -> Result<RouteResultDto, IpcError> {
    use nuomi_core::services::capability_router::{route, RouteRequest};
    let path = state.db_path.clone();
    let req = RouteRequest {
        required_capabilities: caps_from_dto(&request.required_capabilities),
        prefer_role_id: request.prefer_role_id,
    };
    let outcome = tokio::task::spawn_blocking(move || -> Result<RouteOutcome, IpcError> {
        let mut db = Db::open(&path)?;
        route(&mut db.0, &req).map_err(map_routing_error)
    })
    .await??;
    Ok(RouteResultDto {
        role: RoleDto::from(outcome.role),
        created_temp: outcome.created_temp,
    })
}

/// Maps capability-router failures onto stable IPC codes.
fn map_routing_error(e: nuomi_core::services::RoutingError) -> IpcError {
    match e {
        nuomi_core::services::RoutingError::NoCapability(caps) => IpcError::with_details(
            "route.no_capability",
            format!("no provider/role covers capabilities: {caps}"),
            serde_json::json!({ "requiredCapabilities": caps }),
        ),
        other => IpcError::new("route.failed", other.to_string()),
    }
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

    let fallback_session = state.kernel.ensure_session_id().await?;
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
        if let Some(ws) = repos::workspace_open_state::find_focused(&db.0)? {
            repos::tasks_runs::bind_run_workspace(&db.0, &run.id, &ws.workspace_id)?;
        }
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
    let mut dto = RunDto::from(prepared.run);
    dto.kind = "team".to_string();
    Ok(dto)
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
    let ws_id = focused_workspace_id(&db_path);

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
                    ws_id,
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
            }?;
            // Run-end GC: remove ephemeral capability-router temp roles
            // (services::capability_router docs).
            if let Err(e) = nuomi_core::services::capability_router::cleanup_expired_temps(&db.0) {
                tracing::warn!(error = %e, run = %rid, "ephemeral role GC failed");
            }
            Ok(())
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
        focused_workspace_id(&state.db_path),
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

// ---------------------------------------------------------------- conversations

pub async fn impl_create_conversation(
    state: &AppState,
    input: ConversationInput,
) -> Result<ConversationDto, IpcError> {
    let kind = ConversationKind::parse(&input.kind).ok_or_else(|| {
        IpcError::new("conversation.invalid_kind", format!("unknown kind: {}", input.kind))
    })?;
    let agent = input
        .agent
        .as_ref()
        .and_then(|a| AgentRefKind::parse(&a.kind).map(|k| (k, a.id.clone())));
    let title = input.title.clone();
    let team_id = input.team_id.clone();
    let path = state.db_path.clone();
    let session = tokio::task::spawn_blocking(move || -> Result<Session, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        // ADR 0013: agent 透传为 participants 写入 conversation_participants。
        let participants: Vec<(AgentRefKind, &str)> = agent
            .as_ref()
            .map(|(k, id)| vec![(*k, id.as_str())])
            .unwrap_or_default();
        let session = nuomi_core::services::create_conversation(
            &db.0,
            kind,
            title.as_deref().unwrap_or(""),
            &participants,
            team_id.as_deref(),
            None,
        )?;
        let active_ws =
            repos::workspace_open_state::find_focused(&db.0)?.map(|r| r.workspace_id).unwrap_or_default();
        if !active_ws.is_empty() {
            repos::sessions::set_workspace_id(&db.0, &session.id, &active_ws)?;
        }
        Ok(session)
    })
    .await
    .map_err(join_err)??;
    Ok(ConversationDto::from(session))
}

pub async fn impl_list_conversations(
    state: &AppState,
    kind: Option<String>,
) -> Result<Vec<ConversationDto>, IpcError> {
    let path = state.db_path.clone();
    let filter = kind.clone();
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<ConversationDto>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let active_ws = repos::workspace_open_state::find_focused(&db.0)?.map(|r| r.workspace_id).unwrap_or_default();
        let filter_ws = if active_ws.is_empty() { "__migrated__" } else { &active_ws };
        let sessions = repos::sessions::list(&db.0, filter_ws, 200)?;
        let mut out = Vec::with_capacity(sessions.len());
        for session in &sessions {
            if filter
                .as_ref()
                .map_or(false, |k| session.kind.as_str() != k.as_str())
            {
                continue;
            }
            let mut dto = ConversationDto::from(session.clone());
            // ADR 0013: 参与者从 conversation_participants 读取，填充真实 name。
            let participants = repos::sessions::list_participants(&db.0, &session.id)?;
            let mut agent_refs = Vec::with_capacity(participants.len());
            for (kind, id) in &participants {
                let name = nuomi_core::services::name_agent_ref(
                    &db.0,
                    Some(&(kind.clone(), id.clone())),
                )?
                .map(|r| r.name)
                .unwrap_or_default();
                agent_refs.push(AgentRefDto {
                    kind: kind.as_str().to_string(),
                    id: id.clone(),
                    name,
                });
            }
            dto.participant_agents = agent_refs;
            out.push(dto);
        }
        Ok(out)
    })
    .await
    .map_err(join_err)??;
    Ok(rows)
}

pub async fn impl_get_conversation(
    state: &AppState,
    session_id: String,
) -> Result<ConversationDto, IpcError> {
    let path = state.db_path.clone();
    let sid = session_id.clone();
    let (session, participant_agents, todos) =
        tokio::task::spawn_blocking(move || -> Result<(Session, Vec<AgentRefDto>, Vec<nuomi_core::domain::TodoItem>), IpcError> {
            let db = Db::open(&path)?;
            migrations::run(&db.0)?;
            let session = repos::sessions::get(&db.0, &sid)?;
            let participants = repos::sessions::list_participants(&db.0, &sid)?;
            // ADR 0013: 参与者填充真实 name（决策 10）。
            let agent_refs: Vec<AgentRefDto> = participants
                .into_iter()
                .map(|(k, id)| {
                    let name = nuomi_core::services::name_agent_ref(
                        &db.0,
                        Some(&(k.clone(), id.clone())),
                    )
                    .ok()
                    .flatten()
                    .map(|r| r.name)
                    .unwrap_or_default();
                    AgentRefDto {
                        kind: k.as_str().to_string(),
                        id,
                        name,
                    }
                })
                .collect();
            let todos = repos::sessions::list_todos(&db.0, &sid)?;
            Ok((session, agent_refs, todos))
        })
        .await
        .map_err(join_err)??;
    let mut dto = ConversationDto::from(session);
    dto.participant_agents = participant_agents;
    dto.todo_list = todos
        .into_iter()
        .map(|t| TodoItemDto {
            id: t.id,
            description: t.description,
            completed: t.completed,
        })
        .collect();
    Ok(dto)
}

pub async fn impl_set_conversation_agent(
    state: &AppState,
    session_id: String,
    agent: Option<AgentRefInput>,
) -> Result<ConversationDto, IpcError> {
    let agent_owned = agent
        .as_ref()
        .and_then(|a| AgentRefKind::parse(&a.kind).map(|k| (k, a.id.clone())));
    let path = state.db_path.clone();
    let sid = session_id.clone();
    let session = tokio::task::spawn_blocking(move || -> Result<Session, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        // ADR 0013: 整体替换参与者列表（单聊切 Role Agent = 替换为 1 人）。
        repos::sessions::clear_participants(&db.0, &sid)?;
        if let Some((kind, id)) = &agent_owned {
            repos::sessions::add_participant(&db.0, &sid, *kind, id, nuomi_core::domain::now_ms())?;
        }
        Ok(repos::sessions::get(&db.0, &sid)?)
    })
    .await
    .map_err(join_err)??;
    Ok(ConversationDto::from(session))
}

// ----------------------------------------------- update_conversation

pub async fn impl_update_conversation(
    state: &AppState,
    session_id: String,
    input: ConversationUpdateInput,
) -> Result<ConversationDto, IpcError> {
    if let Some(ref title) = input.title {
        if title.chars().count() > 100 {
            return Err(IpcError::new(
                "validation",
                "title must be at most 100 characters",
            ));
        }
    }
    if let Some(ref goal) = input.goal {
        if goal.chars().count() > 500 {
            return Err(IpcError::new(
                "validation",
                "goal must be at most 500 characters",
            ));
        }
    }
    let path = state.db_path.clone();
    let sid = session_id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        if let Some(ref title) = input.title {
            repos::sessions::update_title(&db.0, &sid, title)?;
        }
        if input.goal.is_some() {
            let session = repos::sessions::get(&db.0, &sid)?;
            repos::sessions::update_meta(
                &db.0,
                &sid,
                input.goal.as_deref(),
                session.main_agent_id.as_deref(),
                session.route_mode.as_deref(),
                session.whiteboard_route_mode.as_deref(),
            )?;
        }
        Ok(())
    })
    .await
    .map_err(join_err)??;
    impl_get_conversation(state, session_id).await
}

// ----------------------------------------------- add_conversation_agent

pub async fn impl_add_conversation_agent(
    state: &AppState,
    session_id: String,
    agent: AgentRefInput,
) -> Result<ConversationDto, IpcError> {
    let agent_kind = AgentRefKind::parse(&agent.kind)
        .ok_or_else(|| IpcError::new("validation", format!("unknown agent kind: {}", agent.kind)))?;
    let path = state.db_path.clone();
    let sid = session_id.clone();
    let agent_id = agent.id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let session = repos::sessions::get(&db.0, &sid)?;
        if repos::sessions::is_participant(&db.0, &sid, agent_kind, &agent_id)? {
            return Err(IpcError::new(
                "already_exists",
                "agent already in conversation",
            ));
        }
        let now = nuomi_core::domain::now_ms();
        repos::sessions::add_participant(&db.0, &sid, agent_kind, &agent_id, now)?;
        // Single chat → group chat upgrade.
        if session.kind == nuomi_core::domain::ConversationKind::Chat {
            repos::sessions::update_kind(
                &db.0,
                &sid,
                nuomi_core::domain::ConversationKind::Group,
            )?;
        }
        Ok(())
    })
    .await
    .map_err(join_err)??;
    impl_get_conversation(state, session_id).await
}

/// ADR 0013: Removes a participant from a conversation. Group chat → single
/// chat downgrade when participants drop to 1. Single chat's only participant
/// cannot be removed.
pub async fn impl_remove_conversation_agent(
    state: &AppState,
    session_id: String,
    agent: AgentRefInput,
) -> Result<ConversationDto, IpcError> {
    let agent_kind = AgentRefKind::parse(&agent.kind)
        .ok_or_else(|| IpcError::new("validation", format!("unknown agent kind: {}", agent.kind)))?;
    let path = state.db_path.clone();
    let sid = session_id.clone();
    let agent_id = agent.id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let participants = repos::sessions::list_participants(&db.0, &sid)?;
        if participants.len() <= 1 {
            return Err(IpcError::new(
                "validation",
                "cannot remove the last participant",
            ));
        }
        repos::sessions::remove_participant(&db.0, &sid, agent_kind, &agent_id)?;
        // Group chat → single chat downgrade when only 1 participant remains.
        let remaining = repos::sessions::list_participants(&db.0, &sid)?;
        if remaining.len() == 1 {
            let session = repos::sessions::get(&db.0, &sid)?;
            if session.kind == nuomi_core::domain::ConversationKind::Group {
                repos::sessions::update_kind(
                    &db.0,
                    &sid,
                    nuomi_core::domain::ConversationKind::Chat,
                )?;
            }
        }
        Ok(())
    })
    .await
    .map_err(join_err)??;
    impl_get_conversation(state, session_id).await
}

pub async fn impl_delete_conversation(
    state: &AppState,
    session_id: String,
) -> Result<(), IpcError> {
    let path = state.db_path.clone();
    let sid = session_id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        repos::sessions::delete(&db.0, &sid)?;
        // P1-2: cascade — clear any queued messages so the drainer does
        // not consume them for a now-deleted session.
        let _ = repos::message_queue::clear_queued(&db.0, &sid);
        Ok(())
    })
    .await
    .map_err(join_err)??;
    // P1-2: cancel any in-flight turn for this session so a running
    // reply does not keep writing into a deleted conversation.
    state.session_cancels.cancel(&session_id);
    Ok(())
}

pub async fn impl_clear_conversations(state: &AppState) -> Result<usize, IpcError> {
    let path = state.db_path.clone();
    let sids = tokio::task::spawn_blocking(move || -> Result<(usize, Vec<String>), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let active_ws =
            repos::workspace_open_state::find_focused(&db.0)?.map(|r| r.workspace_id).unwrap_or_default();
        let ws_id = if active_ws.is_empty() { "__migrated__" } else { &active_ws };
        // List the sessions about to be soft-deleted so we can cascade
        // queue cleanup + turn cancellation outside the blocking closure.
        let to_delete = repos::sessions::list(&db.0, ws_id, 100_000)?
            .into_iter()
            .map(|s| s.id)
            .collect::<Vec<_>>();
        let count = repos::sessions::delete_all_for_workspace(&db.0, ws_id)?;
        // P1-2: cascade — clear queued messages for every deleted session.
        for sid in &to_delete {
            let _ = repos::message_queue::clear_queued(&db.0, sid);
        }
        Ok((count, to_delete))
    })
    .await
    .map_err(join_err)??;
    // P1-2: cancel any in-flight turns for the deleted sessions.
    for sid in &sids.1 {
        state.session_cancels.cancel(sid);
    }
    Ok(sids.0)
}

// ----------------------------------------------- get_agent_detail

/// 从 Role.params JSON 中读取 `agent_profile_id`（CLI Agent 绑定）。
fn read_role_agent_profile_id(params: &serde_json::Value) -> Option<String> {
    params
        .get("agent_profile_id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// CliFlavor → snake_case 字符串，供 DTO 的 String 字段使用。
fn flavor_to_string(f: CliFlavor) -> String {
    match f {
        CliFlavor::ClaudeCode => "claude_code".to_string(),
        CliFlavor::Codex => "codex".to_string(),
        CliFlavor::Plain => "plain".to_string(),
    }
}

/// Truncates a string to at most `limit` Unicode characters (not bytes),
/// avoiding panics on multi-byte UTF-8 boundaries.
fn truncate_chars(s: &str, limit: usize) -> String {
    if s.chars().count() > limit {
        s.chars().take(limit).collect()
    } else {
        s.to_string()
    }
}

pub async fn impl_get_agent_detail(
    state: &AppState,
    agent_kind: String,
    agent_id: String,
) -> Result<AgentDetailDto, IpcError> {
    let path = state.db_path.clone();
    let result =
        tokio::task::spawn_blocking(move || -> Result<AgentDetailDto, IpcError> {
            let db = Db::open(&path)?;
            migrations::run(&db.0)?;
            match agent_kind.as_str() {
                "cli" => {
                    let p = repos::agent_profiles::get(&db.0, &agent_id)?;
                    Ok(AgentDetailDto {
                        kind: "cli".to_string(),
                        id: p.id.clone(),
                        name: p.name.clone(),
                        avatar_url: None,
                        role: Some(p.adapter.clone()),
                        responsibility: None,
                        bound_model: p.model_id.clone(),
                        provider: None,
                        binding_kind: Some("cli".to_string()),
                        cli_agent_name: Some(p.name.clone()),
                        cli_agent_flavor: Some(flavor_to_string(p.flavor)),
                        cli_agent_model: p.model_id.clone(),
                        enabled: p.enabled,
                    })
                }
                "role" => {
                    let r = repos::roles::get(&db.0, &agent_id)?;
                    let agent_profile_id = read_role_agent_profile_id(&r.params);

                    // Resolve the bound CLI agent profile. If it has been
                    // deleted (NotFound) or any store error occurs, fall back
                    // to the provider-binding branch so the detail page still
                    // opens with the Role's basic info.
                    let cli_profile = agent_profile_id
                        .as_deref()
                        .and_then(|pid| repos::agent_profiles::get(&db.0, pid).ok());

                    let responsibility = r
                        .system_prompt_override
                        .as_deref()
                        .map(|s| truncate_chars(s, 200));

                    if let Some(p) = cli_profile {
                        Ok(AgentDetailDto {
                            kind: "role".to_string(),
                            id: r.id.clone(),
                            name: r.name.clone(),
                            avatar_url: None,
                            role: Some("role".to_string()),
                            responsibility,
                            bound_model: p.model_id.clone(),
                            provider: None,
                            binding_kind: Some("cli".to_string()),
                            cli_agent_name: Some(p.name.clone()),
                            cli_agent_flavor: Some(flavor_to_string(p.flavor)),
                            cli_agent_model: p.model_id.clone(),
                            enabled: true,
                        })
                    } else {
                        Ok(AgentDetailDto {
                            kind: "role".to_string(),
                            id: r.id.clone(),
                            name: r.name.clone(),
                            avatar_url: None,
                            role: Some("role".to_string()),
                            responsibility,
                            // Role 未指定具体 model；model 由 Provider 配置决定。
                            bound_model: None,
                            provider: r.provider_ids.first().cloned(),
                            binding_kind: r.provider_ids.first().map(|_| "provider".to_string()),
                            cli_agent_name: None,
                            cli_agent_flavor: None,
                            cli_agent_model: None,
                            enabled: true,
                        })
                    }
                }
                _ => Err(IpcError::new(
                    "validation",
                    format!("unknown agent kind: {agent_kind}"),
                )),
            }
        })
        .await
        .map_err(join_err)??;
    Ok(result)
}

pub async fn impl_list_agent_options(state: &AppState) -> Result<Vec<AgentOptionDto>, IpcError> {
    let path = state.db_path.clone();
    let result =
        tokio::task::spawn_blocking(move || -> Result<Vec<AgentOptionDto>, IpcError> {
            let db = Db::open(&path)?;
            migrations::run(&db.0)?;
            let mut options = Vec::new();
            for p in repos::agent_profiles::list(&db.0)? {
                options.push(AgentOptionDto {
                    kind: "cli".to_string(),
                    id: p.id.clone(),
                    name: p.name.clone(),
                    enabled: p.enabled,
                    builtin: false,
                    role: Some(p.adapter.clone()),
                    responsibility: None,
                    bound_model: None,
                    provider: None,
                });
            }
            for r in repos::roles::list(&db.0)? {
                options.push(AgentOptionDto {
                    kind: "role".to_string(),
                    id: r.id.clone(),
                    name: r.name.clone(),
                    enabled: true,
                    builtin: r.builtin,
                    role: Some("role".to_string()),
                    responsibility: r.system_prompt_override.as_deref().map(|s| {
                        if s.len() > 200 { s[..200].to_string() } else { s.to_string() }
                    }),
                    bound_model: r.provider_id.clone(),
                    provider: r.provider_ids.first().cloned(),
                });
            }
            Ok(options)
        })
        .await
        .map_err(join_err)??;
    Ok(result)
}

// ---------------------------------------------------------------- attachments

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentDto {
    pub id: String,
    pub session_id: String,
    pub seq: Option<i64>,
    pub kind: String,
    pub name: String,
    pub mime: String,
    pub rel_path: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub created_at: i64,
}

impl From<nuomi_core::domain::Attachment> for AttachmentDto {
    fn from(a: nuomi_core::domain::Attachment) -> Self {
        Self {
            id: a.id,
            session_id: a.session_id,
            seq: a.seq,
            kind: a.kind.as_str().to_string(),
            name: a.name,
            mime: a.mime,
            rel_path: a.rel_path,
            size_bytes: a.size_bytes,
            sha256: a.sha256,
            created_at: a.created_at,
        }
    }
}

// ---------------------------------------------------------------- schedule input

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleInput {
    pub name: String,
    pub cron_expr: String,
    pub target_kind: String,
    pub agent: Option<AgentRefInput>,
    pub team_id: Option<String>,
    pub session_mode: String,
    pub session_id: Option<String>,
    pub auto_dispatch: bool,
    pub task_title: String,
    pub task_description: String,
}

fn parse_schedule_input(input: ScheduleInput) -> Result<Schedule, IpcError> {
    parse_schedule(&input.cron_expr)
        .map_err(|e| IpcError::new("scheduler.bad_expression", e.to_string()))?;
    let target_kind = ScheduleTargetKind::parse(&input.target_kind).ok_or_else(|| {
        IpcError::new(
            "schedule.invalid_target_kind",
            format!("unknown target_kind: {}", input.target_kind),
        )
    })?;
    let session_mode = ScheduleSessionMode::parse(&input.session_mode).ok_or_else(|| {
        IpcError::new(
            "schedule.invalid_session_mode",
            format!("unknown session_mode: {}", input.session_mode),
        )
    })?;
    let agent = input
        .agent
        .as_ref()
        .and_then(|a| AgentRefKind::parse(&a.kind).map(|k| (k, a.id.clone())));
    let now = now_ms();
    Ok(Schedule {
        id: nuomi_core::domain::new_id(),
        name: input.name,
        cron_expr: input.cron_expr,
        task_title: input.task_title,
        task_description: input.task_description,
        enabled: true,
        last_triggered_at: None,
        next_trigger_at: None,
        created_at: now,
        updated_at: now,
        target_kind,
        agent,
        team_id: input.team_id,
        session_mode,
        session_id: input.session_id,
        auto_dispatch: input.auto_dispatch,
    })
}

// ---------------------------------------------------------------- submit_message

pub async fn impl_submit_message(
    state: &AppState,
    session_id: String,
    text: String,
    attachment_ids: Vec<String>,
    route_target_agent_ids: Option<Vec<String>>,
    _context_injection_ids: Option<Vec<String>>,
) -> Result<RunResultDto, IpcError> {
    // Validate @route targets: all must be participants in the current conversation.
    if let Some(ref targets) = route_target_agent_ids {
        if !targets.is_empty() {
            let path = state.db_path.clone();
            let sid = session_id.clone();
            let targets_clone = targets.clone();
            tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
                let db = Db::open(&path)?;
                migrations::run(&db.0)?;
                let participants = repos::sessions::list_participants(&db.0, &sid)?;
                let participant_ids: std::collections::HashSet<&str> =
                    participants.iter().map(|(_, id)| id.as_str()).collect();
                for target in &targets_clone {
                    if !participant_ids.contains(target.as_str()) {
                        return Err(IpcError::new(
                            "invalid_route_target",
                            format!("agent '{target}' is not a participant in this conversation"),
                        ));
                    }
                }
                Ok(())
            })
            .await
            .map_err(join_err)??;
        }
    }
    // Attachments ride along as inline references; the model sees the path
    // inside the workspace and reads it with the normal file tools.
    let composed = compose_with_attachments(state, &session_id, &text, &attachment_ids).await?;
    let result = run_conversation_turn(state, &session_id, &composed).await?;
    bind_attachments_to_user_message(state, &session_id, &attachment_ids).await?;
    Ok(result)
}

/// Loads the referenced attachments and appends `[name](rel_path)` refs to
/// the outgoing message. Unknown ids are ignored (the message still sends).
async fn compose_with_attachments(
    state: &AppState,
    session_id: &str,
    text: &str,
    attachment_ids: &[String],
) -> Result<String, IpcError> {
    if attachment_ids.is_empty() {
        return Ok(text.to_string());
    }
    let path = state.db_path.clone();
    let ids = attachment_ids.to_vec();
    let sid = session_id.to_string();
    let refs = tokio::task::spawn_blocking(move || -> Result<Vec<(String, String)>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let own = repos::attachments::list_by_session(&db.0, &sid)?;
        Ok(own
            .into_iter()
            .filter(|a| ids.contains(&a.id))
            .map(|a| (a.name, a.rel_path))
            .collect())
    })
    .await
    .map_err(join_err)??;

    let borrowed: Vec<(&str, &str)> = refs
        .iter()
        .map(|(name, rel)| (name.as_str(), rel.as_str()))
        .collect();
    Ok(nuomi_core::services::compose_user_message(text, &borrowed))
}

/// Stamps `attachments.seq` with the `events.seq` of the user message this
/// turn just persisted, so an attachment can be traced back to its message.
async fn bind_attachments_to_user_message(
    state: &AppState,
    session_id: &str,
    attachment_ids: &[String],
) -> Result<(), IpcError> {
    if attachment_ids.is_empty() {
        return Ok(());
    }
    let path = state.db_path.clone();
    let ids = attachment_ids.to_vec();
    let sid = session_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let events = repos::events::list_by_aggregate(&db.0, "session", &sid, None)?;
        let user_seq = events.iter().rev().find_map(|e| {
            (e.kind == "message"
                && e.payload
                    .get("role")
                    .and_then(|r| r.as_str())
                    .map(|r| r == "user")
                    .unwrap_or(false))
            .then_some(e.seq)
        });
        match user_seq {
            Some(seq) => {
                for id in &ids {
                    // A stale id is not an error: the message is already sent.
                    let _ = repos::attachments::update_seq(&db.0, id, seq);
                }
                Ok(())
            }
            None => Ok(()),
        }
    })
    .await
    .map_err(join_err)?
}

// ----------------------------------------------- context injection

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContextInjectionDto {
    pub id: String,
    pub session_id: String,
    pub r#type: String,
    pub ref_id: Option<String>,
    pub text: Option<String>,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ContextInjectionInput {
    pub r#type: String,
    pub ref_id: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InjectableSessionDto {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InjectableRuleDto {
    pub id: String,
    pub name: String,
    pub system_prompt: Option<String>,
}

pub async fn impl_inject_context(
    state: &AppState,
    session_id: String,
    input: ContextInjectionInput,
) -> Result<ContextInjectionDto, IpcError> {
    let path = state.db_path.clone();
    let sid = session_id.clone();
    tokio::task::spawn_blocking(move || -> Result<ContextInjectionDto, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let now = now_ms();
        let id = nuomi_core::domain::new_id();
        db.0.execute(
            "INSERT INTO context_injections (id, session_id, type, ref_id, text, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6)",
            rusqlite::params![id, sid, input.r#type, input.ref_id, input.text, now],
        ).map_err(nuomi_core::store::StoreError::from)?;
        Ok(ContextInjectionDto {
            id,
            session_id: sid,
            r#type: input.r#type,
            ref_id: input.ref_id,
            text: input.text,
            status: "active".to_string(),
            created_at: now,
        })
    })
    .await
    .map_err(join_err)?
}

pub async fn impl_list_injectable_sessions(
    state: &AppState,
) -> Result<Vec<InjectableSessionDto>, IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<Vec<InjectableSessionDto>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let active_ws = repos::workspace_open_state::find_focused(&db.0)?.map(|r| r.workspace_id).unwrap_or_default();
        let filter_ws = if active_ws.is_empty() { "__migrated__" } else { &active_ws };
        let sessions = repos::sessions::list(&db.0, filter_ws, 50)?;
        Ok(sessions
            .into_iter()
            .map(|s| InjectableSessionDto {
                id: s.id,
                title: s.title,
                kind: s.kind.as_str().to_string(),
                updated_at: s.updated_at,
            })
            .collect())
    })
    .await
    .map_err(join_err)?
}

pub async fn impl_list_injectable_rules(
    state: &AppState,
) -> Result<Vec<InjectableRuleDto>, IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<Vec<InjectableRuleDto>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let roles = repos::roles::list(&db.0)?;
        Ok(roles
            .into_iter()
            .map(|r| InjectableRuleDto {
                id: r.id,
                name: r.name,
                system_prompt: r.system_prompt_override,
            })
            .collect())
    })
    .await
    .map_err(join_err)?
}

// ----------------------------------------------- voice recognition

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct VoiceAsrConfigDto {
    pub model_source: String,  // "builtin" | "custom"
    pub custom_model_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AsrModelDto {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub builtin: bool,
}

pub async fn impl_transcribe_audio(
    _state: &AppState,
    _audio_base64: String,
    _model_source: Option<String>,
) -> Result<String, IpcError> {
    // Stub: actual ASR engine integration is deferred to the voice input
    // frontend module. Returns an error so the frontend can show a message.
    Err(IpcError::new(
        "asr_not_available",
        "voice recognition is not yet configured; please set up a model in settings",
    ))
}

pub async fn impl_list_asr_models(_state: &AppState) -> Result<Vec<AsrModelDto>, IpcError> {
    // Return the builtin model plus any provider models that support audio.
    Ok(vec![AsrModelDto {
        id: "builtin".to_string(),
        name: "Built-in ASR".to_string(),
        provider: "builtin".to_string(),
        builtin: true,
    }])
}

// ---------------------------------------------------------------- stop / cancel

pub async fn impl_stop_conversation(
    state: &AppState,
    session_id: String,
) -> Result<(), IpcError> {
    // Chat runs have no `runs` row — they are supervised under the session
    // id, so this is the only handle that can actually stop the loop.
    state.session_cancels.cancel(&session_id);
    let path = state.db_path.clone();
    let sid = session_id.clone();
    let run_cancels = state.run_cancels.clone();
    let bus = state.kernel.context().bus();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let runs = repos::tasks_runs::list_active_runs_by_session(&db.0, &sid)?;
        for run in runs {
            // Team/background runs of this session are supervised by run id.
            run_cancels.cancel_by_run(&run.id);
            let current = run.status;
            if transition_run_and_notify(&db.0, Some(&bus), &run.id, current, RunEvent::Cancel, None)
                .is_ok()
            {
                let _ = append_domain_event(
                    &db.0,
                    "run.cancelled",
                    serde_json::json!({ "runId": run.id, "sessionId": sid }),
                    now_ms(),
                );
            }
        }
        Ok(())
    })
    .await
    .map_err(join_err)??;
    Ok(())
}

pub async fn impl_list_active_runs(state: &AppState) -> Result<Vec<RunDto>, IpcError> {
    let path = state.db_path.clone();
    let runs = tokio::task::spawn_blocking(move || -> Result<Vec<RunDto>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let mut all = Vec::new();
        for s in [RunState::Running, RunState::Queued, RunState::AwaitingApproval] {
            for r in repos::tasks_runs::list_runs_by_status(&db.0, s)? {
                all.push(RunDto::from(r));
            }
        }
        Ok(all)
    })
    .await
    .map_err(join_err)??;
    Ok(runs)
}

pub async fn impl_cancel_run(state: &AppState, run_id: String) -> Result<(), IpcError> {
    // Signal the supervised executor first: without this the DB row flips to
    // `cancelled` while the loop keeps streaming and later overwrites the
    // terminal state with `succeeded`.
    state.run_cancels.cancel_by_run(&run_id);
    let path = state.db_path.clone();
    let rid = run_id.clone();
    let bus = state.kernel.context().bus();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let run = repos::tasks_runs::get_run(&db.0, &rid)?;
        transition_run_and_notify(&db.0, Some(&bus), &rid, run.status, RunEvent::Cancel, None)?;
        let _ = append_domain_event(
            &db.0,
            "run.cancelled",
            serde_json::json!({ "runId": rid }),
            now_ms(),
        );
        Ok(())
    })
    .await
    .map_err(join_err)??;
    Ok(())
}

// ---------------------------------------------------------------- attachments CRUD

/// Max accepted attachment size (25 MB), mirrored by the frontend
/// (`lib/conversation/attachmentModel.ts`).
const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;
/// Same budget expressed in base64 characters (4/3 of the byte size).
const MAX_ATTACHMENT_BASE64_LEN: usize = MAX_ATTACHMENT_BYTES / 3 * 4;

/// True for a single path segment safe to embed in a filesystem path:
/// alphanumeric plus `-`/`_`, no separators and no `.` (blocks `..`).
fn is_safe_path_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// MIME allowlist (mirror of the frontend whitelist): the value is used to
/// derive a file extension, so it must not carry arbitrary characters.
fn is_allowed_mime(mime: &str) -> bool {
    const EXACT: &[&str] = &[
        "application/pdf",
        "application/json",
        "application/x-yaml",
        "application/yaml",
        "application/javascript",
        "application/typescript",
        "application/x-sh",
        "application/x-python",
        "application/x-rust",
        "application/x-go",
        "application/x-toml",
        "application/xml",
        "application/csv",
    ];
    if EXACT.contains(&mime) {
        return true;
    }
    mime.starts_with("image/") || mime.starts_with("text/")
}

/// File extension for a whitelisted MIME type. Anything unrecognised falls
/// back to `bin` — never the raw MIME substring.
fn file_ext_for_mime(mime: &str) -> &'static str {
    match mime.rsplit('/').next().unwrap_or("") {
        "png" => "png",
        "jpeg" | "jpg" => "jpg",
        "gif" => "gif",
        "webp" => "webp",
        "svg+xml" => "svg",
        "plain" => "txt",
        "markdown" => "md",
        "json" => "json",
        "pdf" => "pdf",
        "csv" => "csv",
        "xml" => "xml",
        "yaml" | "x-yaml" => "yaml",
        "javascript" | "typescript" | "x-python" | "x-rust" | "x-go" | "x-sh" | "x-toml" => "txt",
        _ => "bin",
    }
}

pub async fn impl_save_attachment(
    state: &AppState,
    session_id: String,
    name: String,
    mime: String,
    data_base64: String,
) -> Result<AttachmentDto, IpcError> {
    use base64::Engine;
    // Session ids are generated by `domain::new_id` — anything else is a
    // forged id, and it lands in a filesystem path below.
    if !is_safe_path_segment(&session_id) {
        return Err(IpcError::new(
            "attachment.invalid_session",
            "session id must be a plain id",
        ));
    }
    if !is_allowed_mime(&mime) {
        return Err(IpcError::new(
            "attachment.invalid_mime",
            format!("mime not allowed: {mime}"),
        ));
    }
    // 4/3 of the base64 length is the decoded size — check before decoding
    // so a huge payload never lands in memory.
    if data_base64.len() > MAX_ATTACHMENT_BASE64_LEN {
        return Err(IpcError::new(
            "attachment.too_large",
            format!("attachment exceeds {} bytes", MAX_ATTACHMENT_BYTES),
        ));
    }
    let data = base64::engine::general_purpose::STANDARD
        .decode(&data_base64)
        .map_err(|e| IpcError::new("attachment.invalid_base64", e.to_string()))?;
    if data.is_empty() {
        return Err(IpcError::new("attachment.empty", "decoded data is empty"));
    }
    if data.len() > MAX_ATTACHMENT_BYTES {
        return Err(IpcError::new(
            "attachment.too_large",
            format!("attachment exceeds {} bytes", MAX_ATTACHMENT_BYTES),
        ));
    }
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let hash = hasher.finalize();
    let sha256 = format!("{:x}", hash);

    let kind = if mime.starts_with("image/") {
        nuomi_core::domain::AttachmentKind::Image
    } else {
        nuomi_core::domain::AttachmentKind::File
    };
    let ext = file_ext_for_mime(&mime);
    let rel_path = format!(".nuomi/attachments/{}/{}.{}", session_id, sha256, ext);
    let size_bytes = data.len() as i64;
    let now = now_ms();
    let id = nuomi_core::domain::new_id();
    let ws = state.current_workspace().to_path_buf();
    let attachment = nuomi_core::domain::Attachment {
        id: id.clone(),
        session_id: session_id.clone(),
        seq: None,
        kind,
        name: name.clone(),
        mime: mime.clone(),
        rel_path: rel_path.clone(),
        size_bytes,
        sha256: sha256.clone(),
        created_at: now,
    };
    let path = state.db_path.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<AttachmentDto, IpcError> {
        let disk_dir = ws.join(format!(".nuomi/attachments/{}", session_id));
        std::fs::create_dir_all(&disk_dir)
            .map_err(|e| IpcError::new("attachment.io_error", e.to_string()))?;
        let disk_path = ws.join(&rel_path);
        // Defence in depth: the write must stay inside the session dir even
        // if a future refactor lets an unsanitised segment through.
        if !disk_path.starts_with(&disk_dir) {
            return Err(IpcError::new(
                "attachment.invalid_path",
                "attachment path escapes the session directory",
            ));
        }
        std::fs::write(&disk_path, &data)
            .map_err(|e| IpcError::new("attachment.io_error", e.to_string()))?;
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        repos::attachments::insert(&db.0, &attachment)?;
        Ok(AttachmentDto::from(attachment))
    })
    .await
    .map_err(join_err)??;
    Ok(result)
}

pub async fn impl_list_attachments(
    state: &AppState,
    session_id: String,
) -> Result<Vec<AttachmentDto>, IpcError> {
    let path = state.db_path.clone();
    let rows = tokio::task::spawn_blocking(move || -> Result<Vec<AttachmentDto>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        Ok(repos::attachments::list_by_session(&db.0, &session_id)?
            .into_iter()
            .map(AttachmentDto::from)
            .collect())
    })
    .await
    .map_err(join_err)??;
    Ok(rows)
}

pub async fn impl_delete_attachment(
    state: &AppState,
    attachment_id: String,
) -> Result<(), IpcError> {
    let path = state.db_path.clone();
    let ws = state.current_workspace().to_path_buf();
    let aid = attachment_id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        let att = repos::attachments::get(&db.0, &aid)?;
        repos::attachments::delete(&db.0, &aid)?;
        let disk_path = ws.join(&att.rel_path);
        let _ = std::fs::remove_file(&disk_path);
        Ok(())
    })
    .await
    .map_err(join_err)??;
    Ok(())
}

// ---------------------------------------------------------------- schedule upsert/update

pub async fn impl_upsert_schedule(
    state: &AppState,
    input: ScheduleInput,
) -> Result<ScheduleDto, IpcError> {
    let schedule = parse_schedule_input(input)?;
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
    .await
    .map_err(join_err)??;
    Ok(ScheduleDto::from(created))
}

pub async fn impl_update_schedule(
    state: &AppState,
    schedule_id: String,
    input: ScheduleInput,
) -> Result<ScheduleDto, IpcError> {
    let mut schedule = parse_schedule_input(input)?;
    schedule.id = schedule_id.clone();
    schedule.updated_at = now_ms();
    let clone = schedule.clone();
    let path = state.db_path.clone();
    let updated = tokio::task::spawn_blocking(move || -> Result<Schedule, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        repos::tasks_runs::update_schedule(&db.0, &schedule)?;
        append_domain_event(
            &db.0,
            "schedule.updated",
            serde_json::json!({ "scheduleId": clone.id }),
            clone.updated_at,
        )?;
        Ok(clone)
    })
    .await
    .map_err(join_err)??;
    Ok(ScheduleDto::from(updated))
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
    pub settings: ProviderSettingsDto,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunDto {
    pub id: String,
    pub task_id: String,
    pub session_id: String,
    pub status: String,
    pub heartbeat_at: i64,
    pub kind: String,
    pub cancelable: bool,
}

impl From<nuomi_core::domain::Run> for RunDto {
    fn from(r: nuomi_core::domain::Run) -> Self {
        let cancelable = matches!(
            r.status,
            nuomi_core::domain::RunState::Running
                | nuomi_core::domain::RunState::Queued
                | nuomi_core::domain::RunState::AwaitingApproval
        );
        Self {
            id: r.id,
            task_id: r.task_id,
            session_id: r.session_id,
            status: r.status.as_str().to_string(),
            heartbeat_at: r.heartbeat_at,
            // Default is a single-agent run; team runs override it (see
            // `impl_run_team_on_task`) — the domain `Run` carries no kind.
            kind: "single".to_string(),
            cancelable,
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
    pub task_description: String,
    pub enabled: bool,
    pub target_kind: String,
    pub agent: Option<AgentRefDto>,
    pub team_id: Option<String>,
    pub session_mode: String,
    pub session_id: Option<String>,
    pub auto_dispatch: bool,
    pub last_triggered_at: Option<i64>,
    pub next_trigger_at: Option<i64>,
}

impl From<Schedule> for ScheduleDto {
    fn from(s: Schedule) -> Self {
        Self {
            id: s.id,
            name: s.name,
            cron_expr: s.cron_expr,
            task_title: s.task_title,
            task_description: s.task_description,
            enabled: s.enabled,
            target_kind: s.target_kind.as_str().to_string(),
            agent: s.agent.map(|(k, id)| AgentRefDto {
                kind: k.as_str().to_string(),
                id,
                name: String::new(),
            }),
            team_id: s.team_id,
            session_mode: s.session_mode.as_str().to_string(),
            session_id: s.session_id,
            auto_dispatch: s.auto_dispatch,
            last_triggered_at: s.last_triggered_at,
            next_trigger_at: s.next_trigger_at,
        }
    }
}

// ---------- workspace registry ----------

/// Workspace contract: the active sandbox root plus whether the workspace has
/// been configured (`app_settings` row exists or `NUOMI_WORKSPACE_ROOT` env
/// was set at boot). `configured=false` gates first-launch setup in the UI.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub root: String,
    pub configured: bool,
}

/// A registered workspace entry as seen by the frontend. `directory_present`
/// is a runtime probe (the root dir may have been deleted out-of-band).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEntryDto {
    pub id: String,
    pub root_path: String,
    pub color_tag: String,
    pub created_at: i64,
    pub is_active: bool,
    pub directory_present: bool,
    /// Pinned flag (always restored on startup).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_pinned: Option<bool>,
    /// Whether this workspace is in the open set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_open: Option<bool>,
    /// Whether this workspace is currently focused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_focused: Option<bool>,
    /// When the workspace was opened (unix-ms), if open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opened_at: Option<i64>,
    /// When the workspace was last focused (unix-ms), if open.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_focused_at: Option<i64>,
}

/// A single open workspace in the open set.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenWorkspaceDto {
    pub workspace_id: String,
    pub opened_at: i64,
    pub last_focused_at: i64,
    pub is_focused: bool,
}

/// Unread indicator for a workspace tab (task completions, failures, pending approvals).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UnreadIndicatorDto {
    pub workspace_id: String,
    pub count: i64,
}

/// The current open set + focused workspace + pinned ids.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenSetDto {
    pub open_workspaces: Vec<OpenWorkspaceDto>,
    pub focused_workspace_id: Option<String>,
    pub pinned_workspace_ids: Vec<String>,
    pub unread_indicators: Vec<UnreadIndicatorDto>,
}

/// Layout snapshot DTO for persistence/restoration.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSnapshotDto {
    pub mode: String,
    pub split_workspace_ids: Option<[String; 2]>,
    pub focused_workspace_id: Option<String>,
    pub captured_at: i64,
}

/// A recent workspace entry.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecentWorkspaceDto {
    pub workspace_id: String,
    pub last_used_at: i64,
    pub is_pinned: bool,
}

/// Result of opening a workspace.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenWorkspaceResult {
    pub workspace_id: String,
}

/// Result of closing a workspace.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CloseWorkspaceResult {
    pub closed_id: String,
    pub new_focused_id: Option<String>,
}

/// Details when close requires confirmation.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CloseConfirmDetails {
    pub workspace_id: String,
    pub reason: String,
    pub dirty_files: Vec<String>,
    pub running_tasks: Vec<String>,
}

/// Result of focusing a workspace.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FocusWorkspaceResult {
    pub workspace_id: String,
}

/// A cross-workspace search result group.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CrossSearchGroupDto {
    pub workspace_id: String,
    pub workspace_name: String,
    pub matches: Vec<FileMatchDto>,
}

/// A single file match.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FileMatchDto {
    pub relative_path: String,
    pub match_type: String,
}

/// Cross-workspace search outcome.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CrossSearchOutcomeDto {
    pub groups: Vec<CrossSearchGroupDto>,
    pub skipped_workspace_ids: Vec<String>,
}

/// File reference DTO (read-only snapshot from another workspace).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FileReferenceDto {
    pub source_workspace_id: String,
    pub source_relative_path: String,
    pub content_snapshot: String,
}

/// Diff result DTO for cross-workspace file comparison.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DiffResultDto {
    pub workspace_a_id: String,
    pub workspace_b_id: String,
    pub file_a_path: String,
    pub file_b_path: String,
    pub content_a: String,
    pub content_b: String,
    pub is_identical: bool,
}

/// Result of removing a workspace: the removed id plus the new active id
/// (None when the registry is now empty).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RemoveWorkspaceResult {
    pub removed_id: String,
    pub new_active_id: Option<String>,
}

/// A session whose `workspace_id` points to a removed workspace.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OrphanSessionDto {
    pub session_id: String,
    pub workspace_id: String,
    pub title: String,
    pub updated_at: i64,
}

/// A workspace isolation violation: a run whose workspace_id differs from
/// its parent task's workspace_id (cross-workspace data leakage).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IsolationViolationDto {
    pub run_id: String,
    pub task_id: String,
    pub run_workspace_id: Option<String>,
    pub task_workspace_id: Option<String>,
}

pub async fn impl_list_workspaces(
    state: &AppState,
) -> Result<Vec<WorkspaceEntryDto>, IpcError> {
    let reg = state.workspace_registry.clone();
    let db_path = state.db_path.clone();
    let entries = tokio::task::spawn_blocking(move || -> Result<Vec<WorkspaceEntryDto>, IpcError> {
        let list = reg.list()?;
        let db = Db::open(&db_path)?;
        let open_state: std::collections::HashMap<String, repos::workspace_open_state::WorkspaceOpenStateRow> =
            repos::workspace_open_state::list(&db.0)?
                .into_iter()
                .map(|r| (r.workspace_id.clone(), r))
                .collect();
        Ok(list
            .into_iter()
            .map(|wp| {
                let os = open_state.get(&wp.entry.id);
                WorkspaceEntryDto {
                    id: wp.entry.id.clone(),
                    root_path: wp.entry.root_path,
                    color_tag: wp.entry.color_tag,
                    created_at: wp.entry.created_at,
                    is_active: wp.entry.is_active,
                    directory_present: wp.directory_present,
                    is_pinned: Some(wp.entry.is_pinned),
                    is_open: Some(os.is_some()),
                    is_focused: Some(os.map(|r| r.is_focused).unwrap_or(false)),
                    opened_at: os.map(|r| r.opened_at),
                    last_focused_at: os.map(|r| r.last_focused_at),
                }
            })
            .collect())
    })
    .await
    .map_err(join_err)??;
    Ok(entries)
}

pub async fn impl_add_workspace(
    state: &AppState,
    path: String,
) -> Result<WorkspaceEntryDto, IpcError> {
    let reg = state.workspace_registry.clone();
    let entry = tokio::task::spawn_blocking(move || reg.register(std::path::Path::new(&path)))
        .await
        .map_err(join_err)??;
    // If this was the first workspace (auto-activated), sync the in-memory root.
    if entry.is_active {
        let _ = state.switch_workspace(PathBuf::from(&entry.root_path));
    }
    let directory_present = PathBuf::from(&entry.root_path).is_dir();
    Ok(WorkspaceEntryDto {
        id: entry.id,
        root_path: entry.root_path,
        color_tag: entry.color_tag,
        created_at: entry.created_at,
        is_active: entry.is_active,
        directory_present,
        is_pinned: Some(entry.is_pinned),
        is_open: Some(entry.is_active),
        is_focused: Some(entry.is_active),
        opened_at: if entry.is_active { Some(nuomi_core::domain::now_ms()) } else { None },
        last_focused_at: if entry.is_active { Some(nuomi_core::domain::now_ms()) } else { None },
    })
}

pub async fn impl_remove_workspace(
    state: &AppState,
    id: String,
) -> Result<RemoveWorkspaceResult, IpcError> {
    let reg = state.workspace_registry.clone();
    let result = tokio::task::spawn_blocking(move || reg.remove(&id))
        .await
        .map_err(join_err)??;
    // If activation transferred, sync the in-memory root.
    if result.new_active_id.is_some() {
        let reg2 = state.workspace_registry.clone();
        let new_entry = tokio::task::spawn_blocking(move || reg2.current_active())
            .await
            .map_err(join_err)??;
        if let Some(active) = new_entry {
            let _ = state.switch_workspace(PathBuf::from(&active.root_path));
        }
    }
    Ok(RemoveWorkspaceResult {
        removed_id: result.removed_id,
        new_active_id: result.new_active_id,
    })
}

pub async fn impl_activate_workspace(
    state: &AppState,
    id: String,
) -> Result<WorkspaceEntryDto, IpcError> {
    let reg = state.workspace_registry.clone();
    let entry = tokio::task::spawn_blocking(move || reg.activate(&id))
        .await
        .map_err(join_err)??;
    // Update the in-memory workspace root.
    let root = PathBuf::from(&entry.root_path);
    let _ = state.switch_workspace(root.clone());
    // Append a workspace.activated event for audit.
    let db_path = state.db_path.clone();
    let entry_id = entry.id.clone();
    let entry_root = entry.root_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&db_path)?;
        repos::events::append(
            &db.0,
            "domain",
            "global",
            "workspace.activated",
            &serde_json::json!({ "workspaceId": entry_id, "rootPath": entry_root }),
            nuomi_core::domain::now_ms(),
        )?;
        Ok(())
    })
    .await
    .map_err(join_err)??;
    Ok(WorkspaceEntryDto {
        id: entry.id,
        root_path: entry.root_path,
        color_tag: entry.color_tag,
        created_at: entry.created_at,
        is_active: true,
        directory_present: root.is_dir(),
        is_pinned: Some(entry.is_pinned),
        is_open: Some(true),
        is_focused: Some(true),
        opened_at: Some(nuomi_core::domain::now_ms()),
        last_focused_at: Some(nuomi_core::domain::now_ms()),
    })
}

pub async fn impl_get_active_workspace(
    state: &AppState,
) -> Result<Option<WorkspaceEntryDto>, IpcError> {
    let reg = state.workspace_registry.clone();
    let active = tokio::task::spawn_blocking(move || reg.current_active())
        .await
        .map_err(join_err)??;
    Ok(active.map(|entry| {
        let directory_present = PathBuf::from(&entry.root_path).is_dir();
        WorkspaceEntryDto {
            id: entry.id,
            root_path: entry.root_path,
            color_tag: entry.color_tag,
            created_at: entry.created_at,
            is_active: entry.is_active,
            directory_present,
            is_pinned: Some(entry.is_pinned),
            is_open: Some(entry.is_active),
            is_focused: Some(entry.is_active),
            opened_at: None,
            last_focused_at: None,
        }
    }))
}

// ---- Multi-workspace open-set commands ----

pub async fn impl_open_workspace(
    state: &AppState,
    id: String,
) -> Result<OpenWorkspaceResult, IpcError> {
    let db_path = state.db_path.clone();
    let id_for_open = id.clone();
    tokio::task::spawn_blocking(move || -> Result<OpenWorkspaceResult, IpcError> {
        let svc = nuomi_core::services::WorkspaceOpenSetService::new(PathBuf::from(db_path.as_ref()));
        svc.open(&id_for_open).map_err(map_open_set_error)?;
        Ok(OpenWorkspaceResult { workspace_id: id_for_open })
    })
    .await
    .map_err(join_err)??;
    // Sync in-memory root to the opened workspace.
    let reg = state.workspace_registry.clone();
    let id_clone = id.clone();
    let entry = tokio::task::spawn_blocking(move || reg.find_by_id(&id_clone))
        .await
        .map_err(join_err)??;
    if let Some(e) = entry {
        let _ = state.switch_workspace(PathBuf::from(&e.root_path));
    }
    Ok(OpenWorkspaceResult { workspace_id: id })
}

pub async fn impl_close_workspace(
    state: &AppState,
    id: String,
    force: bool,
) -> Result<CloseWorkspaceResult, IpcError> {
    let db_path = state.db_path.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<nuomi_core::services::workspace_open_set::CloseOutcome, IpcError> {
        let svc = nuomi_core::services::WorkspaceOpenSetService::new(PathBuf::from(db_path.as_ref()));
        svc.close(&id, force).map_err(map_open_set_error)
    })
    .await
    .map_err(join_err)??;
    // Sync in-memory root if focus transferred.
    if let Some(ref new_focused) = result.new_focused_id {
        let reg = state.workspace_registry.clone();
        let nid = new_focused.clone();
        let entry = tokio::task::spawn_blocking(move || reg.find_by_id(&nid))
            .await
            .map_err(join_err)??;
        if let Some(e) = entry {
            let _ = state.switch_workspace(PathBuf::from(&e.root_path));
        }
    }
    Ok(CloseWorkspaceResult {
        closed_id: result.closed_id,
        new_focused_id: result.new_focused_id,
    })
}

pub async fn impl_focus_workspace(
    state: &AppState,
    id: String,
) -> Result<FocusWorkspaceResult, IpcError> {
    let db_path = state.db_path.clone();
    let id_for_focus = id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let svc = nuomi_core::services::WorkspaceOpenSetService::new(PathBuf::from(db_path.as_ref()));
        svc.focus(&id_for_focus).map_err(map_open_set_error)
    })
    .await
    .map_err(join_err)??;
    // Sync in-memory root to the focused workspace.
    let reg = state.workspace_registry.clone();
    let id_clone = id.clone();
    let entry = tokio::task::spawn_blocking(move || reg.find_by_id(&id_clone))
        .await
        .map_err(join_err)??;
    if let Some(e) = entry {
        let _ = state.switch_workspace(PathBuf::from(&e.root_path));
    }
    Ok(FocusWorkspaceResult { workspace_id: id })
}

pub async fn impl_close_all_workspaces(
    state: &AppState,
    exclude_pinned: bool,
) -> Result<Vec<CloseWorkspaceResult>, IpcError> {
    let db_path = state.db_path.clone();
    let outcomes = tokio::task::spawn_blocking(move || -> Result<Vec<nuomi_core::services::workspace_open_set::CloseOutcome>, IpcError> {
        let svc = nuomi_core::services::WorkspaceOpenSetService::new(PathBuf::from(db_path.as_ref()));
        svc.close_all(exclude_pinned).map_err(map_open_set_error)
    })
    .await
    .map_err(join_err)??;
    Ok(outcomes
        .into_iter()
        .map(|o| CloseWorkspaceResult {
            closed_id: o.closed_id,
            new_focused_id: o.new_focused_id,
        })
        .collect())
}

pub async fn impl_get_open_set(
    state: &AppState,
) -> Result<OpenSetDto, IpcError> {
    let db_path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<OpenSetDto, IpcError> {
        let db = Db::open(&db_path)?;
        let open_state = repos::workspace_open_state::list(&db.0)?;
        let focused = repos::workspace_open_state::find_focused(&db.0)?;
        let all_workspaces = repos::workspaces::list(&db.0)?;
        let pinned_ids: Vec<String> = all_workspaces
            .iter()
            .filter(|w| w.is_pinned)
            .map(|w| w.id.clone())
            .collect();
        let unread_indicators = open_state
            .iter()
            .filter_map(|r| {
                let count =
                    repos::tasks_runs::count_unread_since(&db.0, &r.workspace_id, r.last_focused_at)
                        .unwrap_or(0);
                if count > 0 {
                    Some(UnreadIndicatorDto {
                        workspace_id: r.workspace_id.clone(),
                        count,
                    })
                } else {
                    None
                }
            })
            .collect();
        Ok(OpenSetDto {
            open_workspaces: open_state
                .into_iter()
                .map(|r| OpenWorkspaceDto {
                    workspace_id: r.workspace_id,
                    opened_at: r.opened_at,
                    last_focused_at: r.last_focused_at,
                    is_focused: r.is_focused,
                })
                .collect(),
            focused_workspace_id: focused.map(|r| r.workspace_id),
            pinned_workspace_ids: pinned_ids,
            unread_indicators,
        })
    })
    .await
    .map_err(join_err)?
}

// ---- Pin/unpin commands ----

pub async fn impl_pin_workspace(
    state: &AppState,
    id: String,
) -> Result<(), IpcError> {
    let reg = state.workspace_registry.clone();
    tokio::task::spawn_blocking(move || reg.pin(&id))
        .await
        .map_err(join_err)??;
    Ok(())
}

pub async fn impl_unpin_workspace(
    state: &AppState,
    id: String,
) -> Result<(), IpcError> {
    let reg = state.workspace_registry.clone();
    tokio::task::spawn_blocking(move || reg.unpin(&id))
        .await
        .map_err(join_err)??;
    Ok(())
}

// ---- Layout snapshot commands ----

pub async fn impl_get_layout_snapshot(
    state: &AppState,
) -> Result<Option<LayoutSnapshotDto>, IpcError> {
    let db_path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<Option<LayoutSnapshotDto>, IpcError> {
        let db = Db::open(&db_path)?;
        let snap = repos::workspace_layout_snapshot::get(&db.0)?;
        Ok(snap.map(|s| LayoutSnapshotDto {
            mode: s.mode.as_str().to_string(),
            split_workspace_ids: s.split_workspace_ids,
            focused_workspace_id: s.focused_workspace_id,
            captured_at: s.captured_at,
        }))
    })
    .await
    .map_err(join_err)?
}

pub async fn impl_set_layout_snapshot(
    state: &AppState,
    mode: String,
    split_workspace_ids: Option<[String; 2]>,
) -> Result<(), IpcError> {
    let db_path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let svc = nuomi_core::services::WorkspaceLayoutService::new(PathBuf::from(db_path.as_ref()));
        let layout_mode = match mode.as_str() {
            "split" => nuomi_core::services::LayoutMode::Split,
            "overview" => nuomi_core::services::LayoutMode::Overview,
            _ => nuomi_core::services::LayoutMode::Single,
        };
        svc.capture_snapshot(layout_mode, split_workspace_ids)
            .map_err(|e| IpcError::new("internal", format!("{e}")))?;
        Ok(())
    })
    .await
    .map_err(join_err)?
}

pub async fn impl_get_recent_workspaces(
    state: &AppState,
    limit: u32,
) -> Result<Vec<RecentWorkspaceDto>, IpcError> {
    let db_path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<Vec<RecentWorkspaceDto>, IpcError> {
        let db = Db::open(&db_path)?;
        let entries = repos::workspace_recent::list(&db.0, limit as usize)?;
        Ok(entries
            .into_iter()
            .map(|e| RecentWorkspaceDto {
                workspace_id: e.workspace_id,
                last_used_at: e.last_used_at,
                is_pinned: e.is_pinned,
            })
            .collect())
    })
    .await
    .map_err(join_err)?
}

// ---- Cross-workspace commands ----

pub async fn impl_cross_workspace_search(
    state: &AppState,
    query: String,
    match_content: bool,
) -> Result<CrossSearchOutcomeDto, IpcError> {
    let db_path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<CrossSearchOutcomeDto, IpcError> {
        let svc = nuomi_core::services::CrossWorkspaceService::new(PathBuf::from(db_path.as_ref()));
        let outcome = svc.search_all_open(&query, match_content).map_err(|e| IpcError::new("internal", format!("{e}")))?;
        Ok(CrossSearchOutcomeDto {
            groups: outcome
                .groups
                .into_iter()
                .map(|g| CrossSearchGroupDto {
                    workspace_id: g.workspace_id,
                    workspace_name: g.workspace_name,
                    matches: g
                        .matches
                        .into_iter()
                        .map(|m| FileMatchDto {
                            relative_path: m.relative_path,
                            match_type: match m.match_type {
                                nuomi_core::services::MatchType::FileName => "fileName".into(),
                                nuomi_core::services::MatchType::FileContent => "fileContent".into(),
                            },
                        })
                        .collect(),
                })
                .collect(),
            skipped_workspace_ids: outcome.skipped_workspace_ids,
        })
    })
    .await
    .map_err(join_err)?
}

pub async fn impl_cross_workspace_reference(
    state: &AppState,
    source_workspace_id: String,
    file_path: String,
) -> Result<FileReferenceDto, IpcError> {
    let db_path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<FileReferenceDto, IpcError> {
        let svc = nuomi_core::services::CrossWorkspaceService::new(PathBuf::from(db_path.as_ref()));
        let ref_ = svc
            .create_file_reference(&source_workspace_id, &file_path)
            .map_err(|e| IpcError::new("internal", format!("{e}")))?;
        Ok(FileReferenceDto {
            source_workspace_id: ref_.source_workspace_id,
            source_relative_path: ref_.source_relative_path,
            content_snapshot: ref_.content_snapshot,
        })
    })
    .await
    .map_err(join_err)?
}

pub async fn impl_cross_workspace_compare(
    state: &AppState,
    workspace_a: String,
    file_a: String,
    workspace_b: String,
    file_b: String,
) -> Result<DiffResultDto, IpcError> {
    let db_path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<DiffResultDto, IpcError> {
        let svc = nuomi_core::services::CrossWorkspaceService::new(PathBuf::from(db_path.as_ref()));
        let diff = svc
            .compare_files(&workspace_a, &file_a, &workspace_b, &file_b)
            .map_err(|e| IpcError::new("internal", format!("{e}")))?;
        Ok(DiffResultDto {
            workspace_a_id: diff.workspace_a_id,
            workspace_b_id: diff.workspace_b_id,
            file_a_path: diff.file_a_path,
            file_b_path: diff.file_b_path,
            content_a: diff.content_a,
            content_b: diff.content_b,
            is_identical: diff.is_identical,
        })
    })
    .await
    .map_err(join_err)?
}

/// Maps `OpenSetError` to `IpcError`.
fn map_open_set_error(e: nuomi_core::services::OpenSetError) -> IpcError {
    use nuomi_core::services::OpenSetError as E;
    match e {
        E::NotFound(id) => IpcError::new("workspace.not_found", format!("workspace not found: {id}")),
        E::AlreadyOpen(id) => IpcError::new("workspace.already_open", format!("workspace already open: {id}")),
        E::OpenSetFull(n) => IpcError::new("workspace.open_set_full", format!("open set full (max {n})")),
        E::DirectoryMissing(p) => IpcError::new("workspace.directory_missing", format!("directory missing: {p}")),
        E::ProbeTimeout(ms) => IpcError::new("workspace.probe_timeout", format!("directory probe timed out: {ms}ms")),
        E::Store(e) => IpcError::from(e),
        E::NeedConfirm { workspace_id, reason, dirty_files, running_tasks } => {
            IpcError::with_details(
                "workspace.need_confirm",
                format!("close needs confirmation for workspace {workspace_id}: {reason}"),
                serde_json::json!({
                    "workspaceId": workspace_id,
                    "dirtyFiles": dirty_files,
                    "runningTasks": running_tasks,
                }),
            )
        }
    }
}

pub async fn impl_list_orphan_sessions(
    state: &AppState,
) -> Result<Vec<OrphanSessionDto>, IpcError> {
    let db_path = state.db_path.clone();
    let orphans = tokio::task::spawn_blocking(move || -> Result<Vec<OrphanSessionDto>, IpcError> {
        let db = Db::open(&db_path)?;
        let mut stmt = db.0.prepare(
            "SELECT s.id, s.workspace_id, s.title, s.updated_at
             FROM sessions s
             WHERE s.workspace_id NOT IN (SELECT id FROM workspaces)
               AND s.deleted_at IS NULL
             ORDER BY s.updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(OrphanSessionDto {
                session_id: row.get(0)?,
                workspace_id: row.get(1)?,
                title: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                updated_at: row.get(3)?,
            })
        })?;
        let collected: Result<Vec<_>, rusqlite::Error> = rows.collect();
        Ok(collected.map_err(StoreError::Sqlite)?)
    })
    .await
    .map_err(join_err)??;
    Ok(orphans)
}

pub async fn impl_detect_isolation_violations(
    state: &AppState,
) -> Result<Vec<IsolationViolationDto>, IpcError> {
    let db_path = state.db_path.clone();
    let violations =
        tokio::task::spawn_blocking(move || -> Result<Vec<IsolationViolationDto>, IpcError> {
            let db = Db::open(&db_path)?;
            let raw = repos::tasks_runs::detect_isolation_violations(&db.0)?;
            Ok(raw
                .into_iter()
                .map(|v| IsolationViolationDto {
                    run_id: v.run_id,
                    task_id: v.task_id,
                    run_workspace_id: v.run_workspace_id,
                    task_workspace_id: v.task_workspace_id,
                })
                .collect())
        })
        .await
        .map_err(join_err)??;
    Ok(violations)
}

pub async fn impl_reclaim_orphan_sessions(
    state: &AppState,
    workspace_id: String,
) -> Result<u64, IpcError> {
    let db_path = state.db_path.clone();
    let target_id = workspace_id.clone();
    // Verify the target workspace exists.
    let check_path = state.db_path.clone();
    let check_id = workspace_id.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&check_path)?;
        if repos::workspaces::find_by_id(&db.0, &check_id)?.is_none() {
            return Err(IpcError::new(
                "workspace.not_found",
                format!("workspace #{check_id} not found"),
            ));
        }
        Ok(())
    })
    .await
    .map_err(join_err)??;
    let count = tokio::task::spawn_blocking(move || -> Result<u64, IpcError> {
        let db = Db::open(&db_path)?;
        let n = db.0.execute(
            "UPDATE sessions SET workspace_id = ?1
             WHERE workspace_id NOT IN (SELECT id FROM workspaces)",
            rusqlite::params![target_id],
        )?;
        Ok(n as u64)
    })
    .await
    .map_err(join_err)??;
    Ok(count)
}

// ---------- workspace switch (legacy compat) ----------

pub async fn impl_get_workspace(state: &AppState) -> Result<WorkspaceInfo, IpcError> {
    // Compose from the new registry: root from the active workspace (or the
    // in-memory fallback), configured = registry non-empty OR env pinned.
    let active = impl_get_active_workspace(state).await?;
    let root = active
        .as_ref()
        .map(|ws| ws.root_path.clone())
        .unwrap_or_else(|| state.current_workspace().to_string_lossy().to_string());
    let configured = active.is_some() || state.workspace_env_configured || workspace_persisted(state)?;
    Ok(WorkspaceInfo { root, configured })
}

pub async fn impl_set_workspace(state: &AppState, path: String) -> Result<WorkspaceInfo, IpcError> {
    // Legacy compat: if the path is already registered, just activate it.
    // Otherwise register (which auto-activates if first) then activate.
    let reg = state.workspace_registry.clone();
    let existing = {
        let p = path.clone();
        tokio::task::spawn_blocking(move || reg.find_by_path(std::path::Path::new(&p)))
            .await
            .map_err(join_err)??
    };
    let target_id = if let Some(entry) = existing {
        entry.id
    } else {
        let ws = impl_add_workspace(state, path.clone()).await?;
        ws.id
    };
    impl_activate_workspace(state, target_id).await?;
    Ok(WorkspaceInfo {
        root: state.current_workspace().to_string_lossy().to_string(),
        configured: true,
    })
}

/// Whether a persisted workspace root row exists (blocking call context).
fn workspace_persisted(state: &AppState) -> Result<bool, IpcError> {
    let db = Db::open(&state.db_path)?;
    Ok(repos::settings::get(&db.0, repos::settings::WORKSPACE_ROOT)?.is_some())
}

/// Time-travel rollback of a Harness Journal `Applied` entry. The journal is
/// reloaded from its conventional root (`<db_parent>/evolution-journal`) and
/// the before-snapshot carried by the entry is restored through the
/// versioning manager — a fresh manager suffices because the snapshot travels
/// inside the journal entry payload.
pub async fn impl_journal_rollback(state: &AppState, seq: u64) -> Result<(), IpcError> {
    use nuomi_core::evolution::journal::EvolutionJournal;
    use nuomi_core::evolution::versioning::PromptVersionManager;

    let db_path = state.db_path.clone();
    let root = std::path::Path::new(&*db_path)
        .parent()
        .map(|p| p.join("evolution-journal"))
        .unwrap_or_else(|| std::path::PathBuf::from("evolution-journal"));
    let journal = tokio::task::spawn_blocking(move || EvolutionJournal::new(root, Some(db_path)))
        .await
        .map_err(IpcError::from)?;
    let versions = PromptVersionManager::new(state.db_path.clone());
    journal
        .rollback_to(seq, &versions)
        .await
        .map(|_| ())
        .map_err(|e| IpcError::new("journal.rollback_failed", e.to_string()))
}

// ---------- plugins panel (ADR 0009 §4 addendum) ----------

/// One `[[editor.commands]]` entry (ADR 0010).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EditorCommandDto {
    name: String,
    title: String,
    tool: String,
}

/// One `[[editor.overlays]]` entry (ADR 0010): URL rendered in a sandboxed
/// iframe overlay by the shell.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EditorOverlayDto {
    id: String,
    title: String,
    url: String,
    width: u32,
    height: u32,
}

/// The plugin manifest's `[editor]` section (ADR 0010). Present only when the
/// plugin declares editor contributions.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct EditorContributionDto {
    /// Monaco language ids served; `["*"]` = all.
    languages: Vec<String>,
    /// Plugin implements the `editor/hover` NPP method.
    hover: bool,
    /// Plugin implements the `editor/symbols` NPP method.
    symbols: bool,
    commands: Vec<EditorCommandDto>,
    overlays: Vec<EditorOverlayDto>,
}

impl EditorContributionDto {
    fn from_manifest(editor: &nuomi_core::harness::sideload::manifest::EditorSection) -> Self {
        Self {
            languages: editor.languages.clone(),
            hover: editor.hover,
            symbols: editor.symbols,
            commands: editor
                .commands
                .iter()
                .map(|c| EditorCommandDto {
                    name: c.name.clone(),
                    title: c.title.clone(),
                    tool: c.tool.clone(),
                })
                .collect(),
            overlays: editor
                .overlays
                .iter()
                .map(|o| EditorOverlayDto {
                    id: o.id.clone(),
                    title: o.title.clone(),
                    url: o.url.clone(),
                    width: o.width,
                    height: o.height,
                })
                .collect(),
        }
    }
}

/// Declared permission surface of one plugin (panel display only, v1).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginPermissionsDto {
    fs_read: Vec<String>,
    fs_write: Vec<String>,
    network: Vec<String>,
    shell: bool,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfoDto {
    id: String,
    name: String,
    version: String,
    api_version: u32,
    description: Option<String>,
    /// env | config | user | workspace (loader SourceKind label).
    source: String,
    /// Absolute plugin directory (display + "reveal" affordances).
    dir: String,
    /// True when the panel may uninstall it (source = user config dir).
    uninstallable: bool,
    /// Fully-qualified tool names (`<id>.<tool>`).
    tools: Vec<String>,
    hooks: Vec<String>,
    events: Vec<String>,
    /// Editor contributions (ADR 0010); `None` when the manifest has no
    /// `[editor]` section.
    editor: Option<EditorContributionDto>,
    permissions: PluginPermissionsDto,
}

#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginListResultDto {
    plugins: Vec<PluginInfoDto>,
    skipped: Vec<String>,
    failed: Vec<String>,
}

fn plugin_info_from_manifest(
    manifest: &nuomi_core::harness::sideload::PluginManifest,
    source: &str,
    dir: &std::path::Path,
) -> PluginInfoDto {
    PluginInfoDto {
        id: manifest.id.clone(),
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        api_version: manifest.api_version,
        description: manifest.description.clone(),
        source: source.to_string(),
        dir: dir.display().to_string(),
        uninstallable: source == "user",
        tools: manifest
            .tools
            .iter()
            .map(|t| format!("{}.{}", manifest.id, t.name))
            .collect(),
        hooks: manifest.hooks.iter().map(|h| h.point.clone()).collect(),
        events: manifest.events.iter().map(|e| e.topic.clone()).collect(),
        editor: manifest
            .editor
            .as_ref()
            .map(EditorContributionDto::from_manifest),
        permissions: PluginPermissionsDto {
            fs_read: manifest.permissions.fs.read.clone(),
            fs_write: manifest.permissions.fs.write.clone(),
            network: manifest.permissions.network.clone(),
            shell: manifest.permissions.shell,
        },
    }
}

/// Disk scan of every side-load location — what the NEXT boot would load.
/// Pure read; no kernel or database involved.
pub fn impl_plugin_list() -> PluginListResultDto {
    let outcomes = nuomi_core::harness::sideload::scan(&[]);
    let mut plugins = Vec::new();
    let mut skipped = Vec::new();
    let mut failed = Vec::new();
    for outcome in outcomes {
        match outcome {
            nuomi_core::harness::sideload::LoadOutcome::Loaded {
                source,
                dir,
                manifest,
            } => {
                plugins.push(plugin_info_from_manifest(&manifest, source.label(), &dir));
            }
            nuomi_core::harness::sideload::LoadOutcome::Skipped { dir, reason } => {
                skipped.push(format!("{}: {reason}", dir.display()));
            }
            nuomi_core::harness::sideload::LoadOutcome::Failed { dir, reason } => {
                failed.push(format!("{}: {reason}", dir.display()));
            }
        }
    }
    PluginListResultDto {
        plugins,
        skipped,
        failed,
    }
}

/// Installs from a plugin directory or `.zip` into the user plugin dir.
/// Takes effect on next app boot (the running kernel keeps its process set).
pub fn impl_plugin_install_from_path(path: String) -> Result<PluginInfoDto, IpcError> {
    let plugins_dir = nuomi_core::harness::sideload::user_plugins_dir()
        .map_err(|e| IpcError::new("plugin.install_failed", e.to_string()))?;
    let manifest =
        nuomi_core::harness::sideload::install_into(&plugins_dir, std::path::Path::new(&path))
            .map_err(|e| IpcError::new("plugin.install_failed", e.to_string()))?;
    let dir = plugins_dir.join(&manifest.id);
    Ok(plugin_info_from_manifest(&manifest, "user", &dir))
}

/// Uninstalls a user-managed plugin by id (removes its directory).
pub fn impl_plugin_uninstall(plugin_id: String) -> Result<(), IpcError> {
    let plugins_dir = nuomi_core::harness::sideload::user_plugins_dir()
        .map_err(|e| IpcError::new("plugin.uninstall_failed", e.to_string()))?;
    nuomi_core::harness::sideload::uninstall_from(&plugins_dir, &plugin_id)
        .map_err(|e| IpcError::new("plugin.uninstall_failed", e.to_string()))?;
    Ok(())
}

/// Reveals the user plugin directory in the OS file manager (fallback-free
/// convenience so users can hand-edit without hunting for the path).
pub fn impl_plugin_open_dir() -> Result<(), IpcError> {
    let dir = nuomi_core::harness::sideload::user_plugins_dir()
        .map_err(|e| IpcError::new("plugin.open_dir_failed", e.to_string()))?;
    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer").arg(&dir).spawn();
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(&dir).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = std::process::Command::new("xdg-open").arg(&dir).spawn();
    result.map_err(|e| IpcError::new("plugin.open_dir_failed", e.to_string()))?;
    Ok(())
}

/// Forwards one editor RPC to a side-loaded plugin's process over NPP
/// (ADR 0010). Requires a booted kernel and a live plugin; structured errors
/// (`kernel_not_ready` / `plugin_not_loaded`) let the frontend degrade
/// gracefully instead of wedging editor providers.
pub async fn impl_plugin_editor_call(
    state: &AppState,
    plugin_id: String,
    method: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, IpcError> {
    let bridge = state
        .kernel
        .context()
        .service::<nuomi_core::harness::EditorBridgeRegistry>("editor_bridge")
        .await
        .ok_or_else(|| IpcError::new("kernel_not_ready", "kernel is not booted yet"))?;
    bridge
        .call(&plugin_id, &method, params)
        .await
        .map_err(|e| IpcError::new("plugin_not_loaded", e.to_string()))
}

/// Reads one app_settings key (ADR 0010 extension enable state et al).
pub async fn impl_app_setting_get(
    state: &AppState,
    key: String,
) -> Result<Option<String>, IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<Option<String>, IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        repos::settings::get(&db.0, &key)
            .map_err(|e| IpcError::new("settings.get_failed", e.to_string()))
    })
    .await
    .map_err(|e| IpcError::new("settings.get_failed", e.to_string()))?
}

/// Upserts one app_settings key.
pub async fn impl_app_setting_set(
    state: &AppState,
    key: String,
    value: String,
) -> Result<(), IpcError> {
    let path = state.db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<(), IpcError> {
        let db = Db::open(&path)?;
        migrations::run(&db.0)?;
        repos::settings::set(&db.0, &key, &value)
            .map_err(|e| IpcError::new("settings.set_failed", e.to_string()))
    })
    .await
    .map_err(|e| IpcError::new("settings.set_failed", e.to_string()))?
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
