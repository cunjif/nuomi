//! Schedule dispatcher (plan §5.4): subscribes to `schedule.triggered` bus
//! events and, when `auto_dispatch` is set, launches the appropriate executor:
//! - `target_kind=chat`  → session-level Loop Engine via `run_task_in_session`
//! - `target_kind=group` → team run via `impl_run_team_on_task`
//! - `target_kind=task`  → no-op (Board manual/batch run keeps the old path)
//!
//! Failures are logged and never propagated — a missed dispatch leaves the
//! Task in `queued`, where a user can still drive it manually.

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
            match state
                .kernel
                .run_task_in_session(&session_id, &prompt, None)
                .await
            {
                Ok(result) => tracing::info!(
                    task_id = %task_id,
                    session_id = %session_id,
                    steps = result.steps,
                    "schedule dispatch: chat run completed"
                ),
                Err(e) => tracing::warn!(
                    task_id = %task_id,
                    session_id = %session_id,
                    error = %e,
                    "schedule dispatch: chat run failed"
                ),
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
                tracing::warn!(
                    task_id = %task_id,
                    "schedule dispatch: group target missing team_id — skipping"
                );
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
