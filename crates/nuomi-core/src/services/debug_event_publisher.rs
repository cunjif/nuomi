//! Debug event publisher (module D).
//!
//! Emits observability events at materialization / role-overlay / env-
//! fallback points so users can confirm in the Run timeline that their
//! Settings configuration actually took effect (AC10).
//! SPEC: docs/specs/settings-integration/spec.md (AC10)

use std::sync::Arc;

use serde_json::{json, Value};

use crate::harness::{Event, EventBus};
use crate::store::{repos, Db};
use crate::{CoreError, CoreResult};

use super::single_role_materializer::RoleOverlay;

/// Emits `provider.materialized` — a DB Provider was materialized and will
/// back this turn.
pub async fn emit_materialized(
    bus: &EventBus,
    db_path: &Arc<str>,
    session_id: &str,
    provider_id: &str,
) -> CoreResult<()> {
    let payload = json!({
        "providerId": provider_id,
        "source": "db",
    });
    emit(bus, db_path, session_id, "provider.materialized", payload).await
}

/// Emits `role.applied` — a Role overlay was applied before the LLM call.
pub async fn emit_role_applied(
    bus: &EventBus,
    db_path: &Arc<str>,
    session_id: &str,
    role_id: &str,
    overlay: &RoleOverlay,
) -> CoreResult<()> {
    let payload = json!({
        "roleId": role_id,
        "hasSystemPrompt": overlay.system_prompt.is_some(),
        "hasTemperature": overlay.temperature.is_some(),
        "toolAllowlist": overlay.tool_allowlist,
    });
    emit(bus, db_path, session_id, "role.applied", payload).await
}

/// Emits `provider.env_fallback` — no DB Provider config; falling back to
/// the env-injected single provider.
pub async fn emit_env_fallback(
    bus: &EventBus,
    db_path: &Arc<str>,
    session_id: &str,
) -> CoreResult<()> {
    let payload = json!({ "source": "env" });
    emit(bus, db_path, session_id, "provider.env_fallback", payload).await
}

/// Emits `provider.missing` — a Role's provider binding is null and no
/// `agent_profile_id` fallback exists (WARNING level, AC6).
pub async fn emit_provider_missing(
    bus: &EventBus,
    db_path: &Arc<str>,
    session_id: &str,
    role_id: &str,
    role_name: &str,
) -> CoreResult<()> {
    let payload = json!({
        "roleId": role_id,
        "roleName": role_name,
        "message": "This Role has no Provider bound. Please configure one in Settings.",
    });
    emit(bus, db_path, session_id, "provider.missing", payload).await
}

/// Emits `provider.materialize_warning` for each non-fatal skip collected
/// during materialization (AC7 — skip + warning + EventRecord, not hard error).
pub async fn emit_materialize_warnings(
    bus: &EventBus,
    db_path: &Arc<str>,
    session_id: &str,
    warnings: &[String],
) -> CoreResult<()> {
    for warning in warnings {
        let payload = json!({ "warning": warning, "level": "WARNING" });
        emit(
            bus,
            db_path,
            session_id,
            "provider.materialize_warning",
            payload,
        )
        .await?;
    }
    Ok(())
}

async fn emit(
    bus: &EventBus,
    db_path: &Arc<str>,
    session_id: &str,
    topic: &str,
    payload: Value,
) -> CoreResult<()> {
    bus.publish(Event::new(topic, payload.clone()));

    let path = db_path.clone();
    let sid = session_id.to_string();
    let kind = topic.to_string();
    tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
        let db = Db::open(&path)?;
        repos::events::append(
            &db.0,
            "session",
            &sid,
            &kind,
            &payload,
            crate::domain::now_ms(),
        )?;
        Ok(())
    })
    .await
    .map_err(join_err)??;
    Ok(())
}

fn join_err(e: tokio::task::JoinError) -> CoreError {
    CoreError::Store(crate::store::StoreError::Sqlite(
        rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
    ))
}
