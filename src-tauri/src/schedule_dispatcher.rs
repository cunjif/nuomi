//! Schedule dispatcher (plan §5.4): subscribes to `schedule.triggered` bus
//! events and, when `auto_dispatch` is set, launches the appropriate executor:
//! - `target_kind=chat`  → unified dispatch via `run_conversation_turn` (D1)
//! - `target_kind=group` → team run via `impl_run_team_on_task`; with no
//!   pre-selected team, spontaneous formation (P2-F 组 3): plan a team now
//!   and record a pending proposal — zero Role/Team writes until approved
//! - `target_kind=task`  → no-op (Board manual/batch run keeps the old path)
//!
//! Failures are logged and the Task is marked `failed` — a missed dispatch
//! must be visible to the user (AC8, non-silent).

use nuomi_core::harness::Event;
use tokio::sync::broadcast::error::RecvError;

use crate::commands;
use crate::state::AppState;

/// Spawns the dispatcher loop. Call once after the kernel is booted.
pub fn spawn(state: &AppState) {
    let mut rx = state.kernel.context().subscribe();
    let state = state.clone();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => handle_event(&state, event).await,
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "schedule dispatcher lagged behind bus");
                }
                Err(RecvError::Closed) => {
                    tracing::info!("schedule dispatcher stopping — bus closed");
                    break;
                }
            }
        }
    });
}

async fn handle_event(state: &AppState, event: Event) {
    if event.topic != "schedule.triggered" {
        return;
    }

    let auto_dispatch = event.payload.get("auto_dispatch").and_then(|v| v.as_bool());
    if auto_dispatch != Some(true) {
        return;
    }

    let target_kind = event
        .payload
        .get("target_kind")
        .and_then(|v| v.as_str())
        .unwrap_or("task");

    let task_id = event
        .payload
        .get("task_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let session_id = event
        .payload
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    // Same composition as a team run: the description carries the actual
    // instructions, so a title-only prompt would run a hollow task.
    let task_title = event
        .payload
        .get("task_title")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let task_description = event
        .payload
        .get("task_description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let prompt = if task_description.is_empty() {
        task_title.clone()
    } else {
        format!("{task_title}\n{task_description}")
    };

    match target_kind {
        "chat" => {
            if session_id.is_empty() {
                tracing::warn!(
                    task_id = %task_id,
                    "schedule dispatch: chat target missing session_id — skipping"
                );
                return;
            }
            tracing::info!(
                task_id = %task_id,
                session_id = %session_id,
                "schedule dispatch: starting chat run"
            );
            match commands::run_conversation_turn(state, &session_id, &prompt, None).await {
                Ok(result) => tracing::info!(
                    task_id = %task_id,
                    session_id = %session_id,
                    steps = result.steps,
                    "schedule dispatch: chat run completed"
                ),
                Err(e) => {
                    tracing::warn!(
                        task_id = %task_id,
                        session_id = %session_id,
                        error = %e,
                        "schedule dispatch: chat run failed — marking task failed"
                    );
                    mark_task_failed(state, &task_id, &e.to_string()).await;
                }
            }
        }
        "group" => {
            let team_id = event
                .payload
                .get("team_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if team_id.is_empty() {
                // Spontaneous formation (P2-F 组 3): no pre-selected team —
                // plan one now and record a pending proposal; approval
                // commits it via form_team_from_plan and dispatches.
                dispatch_spontaneous_team(state, &task_id, &prompt).await;
                return;
            }
            tracing::info!(
                task_id = %task_id,
                team_id = %team_id,
                "schedule dispatch: starting team run"
            );
            match commands::impl_run_team_on_task(state, task_id.clone(), team_id).await {
                Ok(run) => tracing::info!(
                    run_id = %run.id,
                    "schedule dispatch: team run started"
                ),
                Err(e) => tracing::warn!(
                    task_id = %task_id,
                    error = %e,
                    "schedule dispatch: team run failed to start"
                ),
            }
        }
        "task" => {
            // Legacy Board behaviour: the Task is already queued, the Board
            // UI or `run_all` will pick it up. No automatic dispatch.
        }
        other => {
            tracing::warn!(
                target_kind = other,
                "schedule dispatch: unknown target_kind — skipping"
            );
        }
    }
}

/// Spontaneous team formation for a scheduled trigger without a
/// pre-selected team (P2-F 组 3): runs the planner dry (zero Role/Team
/// writes), records a pending `team_plan` proposal with source
/// `scheduled`, and leaves the Task queued for the approved dispatch.
/// Planning or persistence failures mark the Task failed (AC8).
async fn dispatch_spontaneous_team(state: &AppState, task_id: &str, prompt: &str) {
    if task_id.is_empty() || prompt.trim().is_empty() {
        tracing::warn!(
            task_id = %task_id,
            "schedule dispatch: spontaneous formation missing task/prompt — skipping"
        );
        return;
    }
    let workspace_id = task_workspace_id(state, task_id).await;
    tracing::info!(
        task_id = %task_id,
        "schedule dispatch: planning spontaneous team proposal"
    );
    let draft = nuomi_core::services::draft_team_plan(
        state.db_path.clone(),
        state.secrets.clone(),
        Some(state.current_workspace()),
        prompt,
        vec![task_id.to_string()],
        commands::FORM_TEAM_MAX_MEMBERS,
    )
    .await;
    let draft = match draft {
        Ok(draft) => draft,
        Err(e) => {
            tracing::warn!(
                task_id = %task_id,
                error = %e,
                "schedule dispatch: spontaneous team planning failed — marking task failed"
            );
            mark_task_failed(state, task_id, &e.to_string()).await;
            return;
        }
    };
    match commands::impl_create_team_proposal(state, draft, "scheduled", &workspace_id).await {
        Ok(proposal) => tracing::info!(
            proposal_id = %proposal.id,
            task_id = %task_id,
            "schedule dispatch: spontaneous team proposal recorded"
        ),
        Err(e) => {
            tracing::warn!(
                task_id = %task_id,
                error = %e,
                "schedule dispatch: recording proposal failed — marking task failed"
            );
            mark_task_failed(state, task_id, &e.to_string()).await;
        }
    }
}

/// Reads the workspace of a scheduled Task (the scheduler creates the row
/// with the schedule's workspace); falls back to the sentinel used for
/// pre-workspace rows so the proposal stays visible in the "all" scope.
async fn task_workspace_id(state: &AppState, task_id: &str) -> String {
    let path = state.db_path.clone();
    let tid = task_id.to_string();
    tokio::task::spawn_blocking(move || -> Option<String> {
        let db = nuomi_core::store::Db::open(&path).ok()?;
        nuomi_core::store::repos::tasks_runs::get_task(&db.0, &tid)
            .ok()
            .map(|task| task.workspace_id)
    })
    .await
    .ok()
    .flatten()
    .unwrap_or_else(|| "__migrated__".to_string())
}

/// Marks a scheduled Task as `failed` with the error message (AC8 — non-silent
/// missed dispatch). Best-effort: if the Task row is gone or already terminal,
/// the update is a no-op.
async fn mark_task_failed(state: &AppState, task_id: &str, error_msg: &str) {
    if task_id.is_empty() {
        return;
    }
    let path = state.db_path.clone();
    let tid = task_id.to_string();
    let msg = error_msg.to_string();
    let _ = tokio::task::spawn_blocking(move || -> Result<(), nuomi_core::store::StoreError> {
        let db = nuomi_core::store::Db::open(&path)?;
        let now = nuomi_core::domain::now_ms();
        db.0.execute(
            "UPDATE tasks SET status = 'failed', updated_at = ?1 WHERE id = ?2 AND status = 'queued'",
            rusqlite::params![now, tid],
        )?;
        tracing::warn!(task_id = %tid, error = %msg, "task marked failed");
        Ok(())
    })
    .await;
}
