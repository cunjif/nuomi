//! Steward settings — aggregated read/write for the settings panel.
//!
//! Provides `get_settings` (panel展开时聚合读) and `update_soul` /
//! `update_binding` / `clear_messages` / `batch_delete_messages` (分区写).
//!
//! The "steward main role" (`role_steward_main`, seeded by migration 0035)
//! is the configuration carrier: its `system_prompt_override` is the SOUL.md,
//! and its `provider_id` / `params.agent_profile_id` is the binding.

use std::sync::Arc;

use rusqlite::{params, OptionalExtension};
use tokio::task::spawn_blocking;

use crate::domain::now_ms;
use crate::domain::steward_enums::{CyclePhase, CycleStatus};
use crate::store::repos::{agent_profiles, events, providers, roles, sessions, steward};
use crate::store::{Db, StoreError};

use super::StewardError;

/// The id of the steward main role (seeded by migration 0035).
pub const STEWARD_MAIN_ROLE_ID: &str = "role_steward_main";

/// Maximum length of SOUL.md content (characters).
pub const MAX_SOUL_LENGTH: usize = 16000;

// ============================================================ DTOs

/// Binding mode — mirrors `RoleBindingPanel`'s inference logic.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BindingMode {
    None,
    Provider,
    Cli,
}

/// SOUL.md configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SoulConfig {
    /// The current SOUL.md content (system_prompt_override). `None` = default.
    pub content: Option<String>,
    /// The maximum allowed length.
    pub max_length: u32,
}

/// Provider/CLI Agent binding configuration.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingConfig {
    pub mode: BindingMode,
    pub provider_id: Option<String>,
    pub agent_profile_id: Option<String>,
}

/// Run status state — derived from evolution cycle status.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StewardRunStatusState {
    Idle,
    Running,
    AwaitingApproval,
    Failed,
}

/// Run status info — derived, not persisted.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStatusInfo {
    pub state: StewardRunStatusState,
    /// snake_case key for i18n mapping (e.g. "idle", "running").
    pub label: String,
}

/// Message statistics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageStats {
    pub total: u64,
    pub cleared_at: Option<i64>,
}

/// Todo item DTO (mirrors `TodoItem` without `session_id`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoItemDto {
    pub id: String,
    pub description: String,
    pub completed: bool,
}

/// Aggregated settings DTO — returned by `steward_get_settings`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StewardSettingsDto {
    pub soul: Option<SoulConfig>,
    pub binding: BindingConfig,
    pub run_status: RunStatusInfo,
    pub todo_list: Vec<TodoItemDto>,
    pub message_stats: MessageStats,
}

// ============================================================ Errors

/// Settings-specific errors (mapped to IpcError codes by the command layer).
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("SOUL.md content too long: {0} > {MAX_SOUL_LENGTH}")]
    SoulTooLong(usize),

    #[error("binding target not found: {0}")]
    BindingTargetNotFound(String),

    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("store error: {0}")]
    Store(String),
}

impl From<StoreError> for SettingsError {
    fn from(e: StoreError) -> Self {
        SettingsError::Store(e.to_string())
    }
}

impl From<rusqlite::Error> for SettingsError {
    fn from(e: rusqlite::Error) -> Self {
        SettingsError::Store(format!("sqlite: {e}"))
    }
}

impl From<SettingsError> for StewardError {
    fn from(e: SettingsError) -> Self {
        match e {
            SettingsError::Store(msg) => StewardError::Store(msg),
            other => StewardError::Store(other.to_string()),
        }
    }
}

// ============================================================ Binding helpers

/// Infers the binding mode from a Role, mirroring `RoleBindingPanel`'s logic.
pub fn infer_binding_mode(role: &crate::domain::Role) -> BindingMode {
    if read_agent_profile_id(&role.params).is_some() {
        BindingMode::Cli
    } else if role.provider_id.is_some() {
        BindingMode::Provider
    } else {
        BindingMode::None
    }
}

/// Reads `agent_profile_id` from a Role's params JSON.
pub fn read_agent_profile_id(params: &serde_json::Value) -> Option<String> {
    params
        .get("agent_profile_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Extracts binding config from a Role.
fn binding_from_role(role: &crate::domain::Role) -> BindingConfig {
    BindingConfig {
        mode: infer_binding_mode(role),
        provider_id: role.provider_id.clone(),
        agent_profile_id: read_agent_profile_id(&role.params),
    }
}

// ============================================================ Run status derivation

/// Derives the steward run status from the most recent evolution cycle.
///
/// - Active cycle with `phase = gate` → `AwaitingApproval`
/// - Active cycle (any other phase) → `Running`
/// - Most recent cycle `status = failed` → `Failed`
/// - Otherwise → `Idle`
fn derive_run_status(conn: &rusqlite::Connection) -> RunStatusInfo {
    let result: Result<RunStatusInfo, StoreError> = (|| {
        let cycles = steward::list_cycles(conn, 1)?;
        if let Some(cycle) = cycles.first() {
            let state = if cycle.status == CycleStatus::Running {
                if cycle.phase == CyclePhase::Gate {
                    StewardRunStatusState::AwaitingApproval
                } else {
                    StewardRunStatusState::Running
                }
            } else if cycle.status == CycleStatus::Failed {
                StewardRunStatusState::Failed
            } else {
                StewardRunStatusState::Idle
            };
            Ok(run_status_info(state))
        } else {
            Ok(run_status_info(StewardRunStatusState::Idle))
        }
    })();
    result.unwrap_or_else(|_| run_status_info(StewardRunStatusState::Idle))
}

fn run_status_info(state: StewardRunStatusState) -> RunStatusInfo {
    let label = match state {
        StewardRunStatusState::Idle => "idle",
        StewardRunStatusState::Running => "running",
        StewardRunStatusState::AwaitingApproval => "awaiting_approval",
        StewardRunStatusState::Failed => "failed",
    }
    .to_string();
    RunStatusInfo { state, label }
}

// ============================================================ Message stats

/// Counts messages after the cleared_at point, excluding deleted-marked events.
fn count_messages_after_cleared(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<(u64, Option<i64>), StoreError> {
    let cleared_at: Option<i64> = conn
        .query_row(
            "SELECT cleared_at FROM steward_sessions WHERE id = ?1",
            params![session_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()?
        .flatten();

    let total: i64 = if let Some(ca) = cleared_at {
        conn.query_row(
            "SELECT COUNT(*) FROM events e
             WHERE e.aggregate_type = 'steward' AND e.aggregate_id = ?1
               AND e.kind IN ('steward.message_received', 'steward.message_sent')
               AND e.created_at > ?2
               AND NOT EXISTS (
                   SELECT 1 FROM steward_deleted_events d
                   WHERE d.session_id = ?1 AND d.event_id = CAST(e.id AS TEXT)
               )",
            params![session_id, ca],
            |row| row.get(0),
        )?
    } else {
        conn.query_row(
            "SELECT COUNT(*) FROM events e
             WHERE e.aggregate_type = 'steward' AND e.aggregate_id = ?1
               AND e.kind IN ('steward.message_received', 'steward.message_sent')
               AND NOT EXISTS (
                   SELECT 1 FROM steward_deleted_events d
                   WHERE d.session_id = ?1 AND d.event_id = CAST(e.id AS TEXT)
               )",
            params![session_id],
            |row| row.get(0),
        )?
    };
    Ok((total as u64, cleared_at))
}

// ============================================================ Repo functions

/// Updates `cleared_at` for a steward session. Cannot reset to NULL.
pub fn update_cleared_at(
    conn: &rusqlite::Connection,
    session_id: &str,
    ts: i64,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE steward_sessions SET cleared_at = ?2 WHERE id = ?1",
        params![session_id, ts],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "steward_session",
            id: session_id.to_string(),
        });
    }
    Ok(())
}

/// Batch-inserts deletion markers. `INSERT OR IGNORE` for idempotency.
/// Caller is responsible for transaction wrapping if atomicity is needed.
pub fn insert_deleted_events(
    conn: &rusqlite::Connection,
    session_id: &str,
    event_ids: &[String],
    deleted_at: i64,
) -> Result<(), StoreError> {
    for eid in event_ids {
        conn.execute(
            "INSERT OR IGNORE INTO steward_deleted_events (session_id, event_id, deleted_at)
             VALUES (?1, ?2, ?3)",
            params![session_id, eid, deleted_at],
        )?;
    }
    Ok(())
}

// ============================================================ Audit events

/// Appends a steward audit event (bypasses StewardEventKind to avoid
/// modifying the 17-variant enum, same pattern as kernel.rs).
fn append_audit_event(
    conn: &rusqlite::Connection,
    session_id: &str,
    kind: &str,
    payload: &serde_json::Value,
) -> Result<(), StoreError> {
    events::append(conn, "steward", session_id, kind, payload, now_ms())?;
    Ok(())
}

// ============================================================ Public API

/// Aggregated read for the settings panel.
///
/// Returns `SettingsError::SessionNotFound` if the session does not exist.
/// Degrades gracefully if the steward main role is missing (returns
/// `soul: None, binding: {mode: None}`).
pub async fn get_settings(
    db_path: Arc<str>,
    session_id: &str,
) -> Result<StewardSettingsDto, SettingsError> {
    let session_id = session_id.to_string();
    spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        let conn = &db.0;

        // Verify session exists
        steward::get_steward_session(conn, &session_id)
            .map_err(|_| SettingsError::SessionNotFound(session_id.clone()))?;

        // Read steward main role (degrade gracefully if missing)
        let (soul, binding) = match roles::get(conn, STEWARD_MAIN_ROLE_ID) {
            Ok(role) => {
                let soul = Some(SoulConfig {
                    content: role.system_prompt_override.clone(),
                    max_length: MAX_SOUL_LENGTH as u32,
                });
                let binding = binding_from_role(&role);
                (soul, binding)
            }
            Err(_) => (
                None,
                BindingConfig {
                    mode: BindingMode::None,
                    provider_id: None,
                    agent_profile_id: None,
                },
            ),
        };

        // Run status (degrades to Idle on error)
        let run_status = derive_run_status(conn);

        // Todo list
        let todo_list: Vec<TodoItemDto> = sessions::list_todos(conn, &session_id)
            .unwrap_or_default()
            .into_iter()
            .map(|t| TodoItemDto {
                id: t.id,
                description: t.description,
                completed: t.completed,
            })
            .collect();

        // Message stats
        let (total, cleared_at) = count_messages_after_cleared(conn, &session_id)?;
        let message_stats = MessageStats { total, cleared_at };

        Ok(StewardSettingsDto {
            soul,
            binding,
            run_status,
            todo_list,
            message_stats,
        })
    })
    .await
    .map_err(|e| SettingsError::Store(format!("join error: {e}")))?
}

/// Updates the SOUL.md (system_prompt_override) of the steward main role.
///
/// - Empty string maps to `None` (falls back to default prompt).
/// - Content exceeding `MAX_SOUL_LENGTH` returns `SoulTooLong`.
/// - Publishes `steward.soul_updated` audit event.
pub async fn update_soul(
    db_path: Arc<str>,
    session_id: &str,
    content: &str,
) -> Result<(), SettingsError> {
    if content.len() > MAX_SOUL_LENGTH {
        return Err(SettingsError::SoulTooLong(content.len()));
    }

    let session_id = session_id.to_string();
    let content = content.to_string();
    spawn_blocking(move || {
        let mut db = Db::open(&db_path)?;
        let conn = &mut db.0;
        let tx = conn.transaction()?;

        // Verify session exists
        steward::get_steward_session(&tx, &session_id)
            .map_err(|_| SettingsError::SessionNotFound(session_id.clone()))?;

        // Read current role
        let mut role = roles::get(&tx, STEWARD_MAIN_ROLE_ID)?;

        // Map empty string to None
        let new_override: Option<String> = {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(content.clone())
            }
        };
        let new_length = new_override.as_ref().map(|s| s.len()).unwrap_or(0);

        let old_value = role.system_prompt_override.clone();
        role.system_prompt_override = new_override;
        role.updated_at = now_ms();
        roles::update(&tx, &role)?;

        // Audit event
        append_audit_event(
            &tx,
            &session_id,
            "steward.soul_updated",
            &serde_json::json!({
                "session_id": session_id,
                "old_length": old_value.as_ref().map(|s| s.len()).unwrap_or(0),
                "new_length": new_length,
            }),
        )?;

        tx.commit()?;
        Ok(())
    })
    .await
    .map_err(|e| SettingsError::Store(format!("join error: {e}")))?
}

/// Updates the binding of the steward main role.
///
/// Validates target existence:
/// - Provider mode: `provider_id` must exist in `list_providers`.
/// - CLI mode: `agent_profile_id` must exist in `list_agent_profiles`.
///
/// Submit mapping mirrors `RoleBindingPanel`:
/// - Provider: `provider_id = target, provider_ids = [target], params = {}`
/// - CLI: `provider_id = null, provider_ids = [], params = {agent_profile_id: target}`
/// - None: all null/empty
///
/// Publishes `steward.binding_updated` audit event.
pub async fn update_binding(
    db_path: Arc<str>,
    session_id: &str,
    mode: BindingMode,
    provider_id: Option<&str>,
    agent_profile_id: Option<&str>,
) -> Result<(), SettingsError> {
    let session_id = session_id.to_string();
    let provider_id = provider_id.map(|s| s.to_string());
    let agent_profile_id = agent_profile_id.map(|s| s.to_string());
    spawn_blocking(move || {
        let mut db = Db::open(&db_path)?;
        let conn = &mut db.0;
        let tx = conn.transaction()?;

        // Verify session exists
        steward::get_steward_session(&tx, &session_id)
            .map_err(|_| SettingsError::SessionNotFound(session_id.clone()))?;

        // Validate target existence
        match &mode {
            BindingMode::Provider => {
                let pid = provider_id.as_ref().ok_or_else(|| {
                    SettingsError::BindingTargetNotFound("provider_id is null".into())
                })?;
                let providers = providers::list_providers(&tx)?;
                if !providers.iter().any(|p| p.id == *pid) {
                    return Err(SettingsError::BindingTargetNotFound(format!(
                        "provider#{pid}"
                    )));
                }
            }
            BindingMode::Cli => {
                let aid = agent_profile_id.as_ref().ok_or_else(|| {
                    SettingsError::BindingTargetNotFound("agent_profile_id is null".into())
                })?;
                let profiles = agent_profiles::list(&tx)?;
                if !profiles.iter().any(|p| p.id == *aid) {
                    return Err(SettingsError::BindingTargetNotFound(format!(
                        "agent_profile#{aid}"
                    )));
                }
            }
            BindingMode::None => {}
        }

        // Read current role and apply binding
        let mut role = roles::get(&tx, STEWARD_MAIN_ROLE_ID)?;
        let old_binding = binding_from_role(&role);

        match &mode {
            BindingMode::Provider => {
                role.provider_id = provider_id.clone();
                role.provider_ids = provider_id.iter().cloned().collect();
                role.params = serde_json::json!({});
            }
            BindingMode::Cli => {
                role.provider_id = None;
                role.provider_ids = vec![];
                role.params = serde_json::json!({
                    "agent_profile_id": agent_profile_id,
                });
            }
            BindingMode::None => {
                role.provider_id = None;
                role.provider_ids = vec![];
                role.params = serde_json::json!({});
            }
        }
        role.updated_at = now_ms();
        roles::update(&tx, &role)?;

        // Audit event
        append_audit_event(
            &tx,
            &session_id,
            "steward.binding_updated",
            &serde_json::json!({
                "session_id": session_id,
                "old_mode": format!("{:?}", old_binding.mode),
                "new_mode": format!("{mode:?}"),
            }),
        )?;

        tx.commit()?;
        Ok(())
    })
    .await
    .map_err(|e| SettingsError::Store(format!("join error: {e}")))?
}

/// Clears all messages for a steward session by setting `cleared_at = now()`.
///
/// Subsequent `list_session_messages` calls will filter out events with
/// `created_at <= cleared_at`. Publishes `steward.messages_cleared` audit event.
pub async fn clear_messages(db_path: Arc<str>, session_id: &str) -> Result<(), SettingsError> {
    let session_id = session_id.to_string();
    spawn_blocking(move || {
        let mut db = Db::open(&db_path)?;
        let conn = &mut db.0;
        let tx = conn.transaction()?;

        // Verify session exists
        steward::get_steward_session(&tx, &session_id)
            .map_err(|_| SettingsError::SessionNotFound(session_id.clone()))?;

        let now = now_ms();
        update_cleared_at(&tx, &session_id, now)?;

        append_audit_event(
            &tx,
            &session_id,
            "steward.messages_cleared",
            &serde_json::json!({
                "session_id": session_id,
                "cleared_at": now,
            }),
        )?;

        tx.commit()?;
        Ok(())
    })
    .await
    .map_err(|e| SettingsError::Store(format!("join error: {e}")))?
}

/// Batch-deletes messages by inserting deletion markers.
///
/// `message_ids` should be in `evt-{id}` format (matching `StewardMessage.id`).
/// Publishes `steward.messages_batch_deleted` audit event.
pub async fn batch_delete_messages(
    db_path: Arc<str>,
    session_id: &str,
    message_ids: &[String],
) -> Result<(), SettingsError> {
    if message_ids.is_empty() {
        return Ok(());
    }

    let session_id = session_id.to_string();
    let message_ids = message_ids.to_vec();
    spawn_blocking(move || {
        let mut db = Db::open(&db_path)?;
        let conn = &mut db.0;
        let tx = conn.transaction()?;

        // Verify session exists
        steward::get_steward_session(&tx, &session_id)
            .map_err(|_| SettingsError::SessionNotFound(session_id.clone()))?;

        // Extract numeric event ids from `evt-{id}` format
        let event_ids: Vec<String> = message_ids
            .iter()
            .filter_map(|mid| mid.strip_prefix("evt-").map(|s| s.to_string()))
            .collect();

        let now = now_ms();
        insert_deleted_events(&tx, &session_id, &event_ids, now)?;

        append_audit_event(
            &tx,
            &session_id,
            "steward.messages_batch_deleted",
            &serde_json::json!({
                "session_id": session_id,
                "deleted_count": event_ids.len(),
                "deleted_ids": message_ids,
            }),
        )?;

        tx.commit()?;
        Ok(())
    })
    .await
    .map_err(|e| SettingsError::Store(format!("join error: {e}")))?
}

/// Reads the steward main role's `system_prompt_override` for SOUL.md
/// injection into `handle_message`. Returns `None` if the role doesn't
/// exist or has no override.
pub fn read_soul_override(conn: &rusqlite::Connection) -> Option<String> {
    roles::get(conn, STEWARD_MAIN_ROLE_ID)
        .ok()
        .and_then(|r| r.system_prompt_override)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    fn db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    fn ensure_session(conn: &rusqlite::Connection, id: &str) {
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at, kind)
             VALUES (?1, 'test', 1, 1, 'background')",
            params![id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO steward_ai (id, ready, online_authorized, created_at)
             VALUES ('sai1', 1, 0, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO steward_sessions (id, steward_id, title, created_at, updated_at)
             VALUES (?1, 'sai1', 'test', 1, 1)",
            params![id],
        )
        .unwrap();
    }

    #[test]
    fn steward_main_role_seeded_by_migration() {
        let conn = db();
        let role = roles::get(&conn, STEWARD_MAIN_ROLE_ID).unwrap();
        assert_eq!(role.name, "Steward: Main");
        assert!(role.builtin);
        assert!(role.system_prompt_override.is_none());
    }

    #[test]
    fn infer_binding_mode_none_for_unbound() {
        let conn = db();
        let role = roles::get(&conn, STEWARD_MAIN_ROLE_ID).unwrap();
        assert_eq!(infer_binding_mode(&role), BindingMode::None);
    }

    #[test]
    fn read_soul_override_none_by_default() {
        let conn = db();
        assert!(read_soul_override(&conn).is_none());
    }

    #[test]
    fn read_soul_override_returns_value_after_update() {
        let conn = db();
        let mut role = roles::get(&conn, STEWARD_MAIN_ROLE_ID).unwrap();
        role.system_prompt_override = Some("custom soul".into());
        role.updated_at = now_ms();
        roles::update(&conn, &role).unwrap();
        assert_eq!(read_soul_override(&conn).as_deref(), Some("custom soul"));
    }

    #[test]
    fn update_cleared_at_sets_timestamp() {
        let conn = db();
        ensure_session(&conn, "sess1");
        update_cleared_at(&conn, "sess1", 12345).unwrap();
        let ca: i64 = conn
            .query_row(
                "SELECT cleared_at FROM steward_sessions WHERE id = 'sess1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ca, 12345);
    }

    #[test]
    fn update_cleared_at_not_found() {
        let conn = db();
        let result = update_cleared_at(&conn, "nonexistent", 123);
        assert!(result.is_err());
    }

    #[test]
    fn insert_deleted_events_marks_events() {
        let conn = db();
        ensure_session(&conn, "sess1");
        insert_deleted_events(&conn, "sess1", &["1".into(), "2".into()], 100).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM steward_deleted_events WHERE session_id = 'sess1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn insert_deleted_events_idempotent() {
        let conn = db();
        ensure_session(&conn, "sess1");
        insert_deleted_events(&conn, "sess1", &["1".into()], 100).unwrap();
        insert_deleted_events(&conn, "sess1", &["1".into()], 200).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM steward_deleted_events WHERE session_id = 'sess1' AND event_id = '1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "INSERT OR IGNORE should be idempotent");
    }

    #[test]
    fn count_messages_after_cleared_no_messages() {
        let conn = db();
        ensure_session(&conn, "sess1");
        let (total, cleared_at) = count_messages_after_cleared(&conn, "sess1").unwrap();
        assert_eq!(total, 0);
        assert!(cleared_at.is_none());
    }

    #[test]
    fn derive_run_status_idle_when_no_cycles() {
        let conn = db();
        let status = derive_run_status(&conn);
        assert_eq!(status.state, StewardRunStatusState::Idle);
    }

    #[test]
    fn derive_run_status_running_for_active_cycle() {
        let conn = db();
        let cycle = crate::domain::EvolutionCycle {
            id: "c1".into(),
            trigger_source: crate::domain::steward_enums::TriggerSource::User,
            trigger_context: "ctx".into(),
            phase: CyclePhase::Cleanse,
            status: CycleStatus::Running,
            created_at: 1,
            ended_at: None,
        };
        steward::insert_cycle(&conn, &cycle).unwrap();
        let status = derive_run_status(&conn);
        assert_eq!(status.state, StewardRunStatusState::Running);
    }

    #[test]
    fn derive_run_status_awaiting_approval_for_gate_phase() {
        let conn = db();
        let cycle = crate::domain::EvolutionCycle {
            id: "c1".into(),
            trigger_source: crate::domain::steward_enums::TriggerSource::User,
            trigger_context: "ctx".into(),
            phase: CyclePhase::Gate,
            status: CycleStatus::Running,
            created_at: 1,
            ended_at: None,
        };
        steward::insert_cycle(&conn, &cycle).unwrap();
        let status = derive_run_status(&conn);
        assert_eq!(status.state, StewardRunStatusState::AwaitingApproval);
    }

    #[test]
    fn derive_run_status_failed_for_failed_cycle() {
        let conn = db();
        let cycle = crate::domain::EvolutionCycle {
            id: "c1".into(),
            trigger_source: crate::domain::steward_enums::TriggerSource::User,
            trigger_context: "ctx".into(),
            phase: CyclePhase::Cleanse,
            status: CycleStatus::Failed,
            created_at: 1,
            ended_at: Some(2),
        };
        steward::insert_cycle(&conn, &cycle).unwrap();
        let status = derive_run_status(&conn);
        assert_eq!(status.state, StewardRunStatusState::Failed);
    }

    #[tokio::test]
    async fn get_settings_returns_dto_for_valid_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_string_lossy().to_string();
        let db_conn = Db::open(&path_str).unwrap();
        migrations::run(&db_conn.0).unwrap();
        drop(db_conn);
        let dbp: Arc<str> = Arc::from(path_str);

        // Create a steward session
        {
            let db = Db::open(&dbp).unwrap();
            db.0.execute(
                "INSERT INTO sessions (id, title, created_at, updated_at, kind)
                 VALUES ('sess1', 'test', 1, 1, 'background')",
                [],
            )
            .unwrap();
            db.0.execute(
                "INSERT INTO steward_ai (id, ready, online_authorized, created_at)
                 VALUES ('sai1', 1, 0, 1)",
                [],
            )
            .unwrap();
            db.0.execute(
                "INSERT INTO steward_sessions (id, steward_id, title, created_at, updated_at)
                 VALUES ('sess1', 'sai1', 'test', 1, 1)",
                [],
            )
            .unwrap();
        }

        let settings = get_settings(dbp, "sess1").await.unwrap();
        assert!(settings.soul.is_some());
        assert_eq!(settings.binding.mode, BindingMode::None);
        assert_eq!(settings.run_status.state, StewardRunStatusState::Idle);
        assert!(settings.todo_list.is_empty());
        assert_eq!(settings.message_stats.total, 0);
    }

    #[tokio::test]
    async fn get_settings_session_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_string_lossy().to_string();
        let db_conn = Db::open(&path_str).unwrap();
        migrations::run(&db_conn.0).unwrap();
        drop(db_conn);
        let dbp: Arc<str> = Arc::from(path_str);

        let result = get_settings(dbp, "nonexistent").await;
        assert!(matches!(result, Err(SettingsError::SessionNotFound(_))));
    }

    #[tokio::test]
    async fn update_soul_writes_to_role() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_string_lossy().to_string();
        let db_conn = Db::open(&path_str).unwrap();
        migrations::run(&db_conn.0).unwrap();
        drop(db_conn);
        let dbp: Arc<str> = Arc::from(path_str);

        // Create a steward session
        {
            let db = Db::open(&dbp).unwrap();
            db.0.execute(
                "INSERT INTO sessions (id, title, created_at, updated_at, kind)
                 VALUES ('sess1', 'test', 1, 1, 'background')",
                [],
            )
            .unwrap();
            db.0.execute(
                "INSERT INTO steward_ai (id, ready, online_authorized, created_at)
                 VALUES ('sai1', 1, 0, 1)",
                [],
            )
            .unwrap();
            db.0.execute(
                "INSERT INTO steward_sessions (id, steward_id, title, created_at, updated_at)
                 VALUES ('sess1', 'sai1', 'test', 1, 1)",
                [],
            )
            .unwrap();
        }

        update_soul(dbp.clone(), "sess1", "You are a helpful steward.")
            .await
            .unwrap();

        // Verify the role was updated
        let db = Db::open(&dbp).unwrap();
        let role = roles::get(&db.0, STEWARD_MAIN_ROLE_ID).unwrap();
        assert_eq!(
            role.system_prompt_override.as_deref(),
            Some("You are a helpful steward.")
        );
    }

    #[tokio::test]
    async fn update_soul_empty_string_maps_to_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_string_lossy().to_string();
        let db_conn = Db::open(&path_str).unwrap();
        migrations::run(&db_conn.0).unwrap();
        drop(db_conn);
        let dbp: Arc<str> = Arc::from(path_str);

        {
            let db = Db::open(&dbp).unwrap();
            db.0.execute(
                "INSERT INTO sessions (id, title, created_at, updated_at, kind)
                 VALUES ('sess1', 'test', 1, 1, 'background')",
                [],
            )
            .unwrap();
            db.0.execute(
                "INSERT INTO steward_ai (id, ready, online_authorized, created_at)
                 VALUES ('sai1', 1, 0, 1)",
                [],
            )
            .unwrap();
            db.0.execute(
                "INSERT INTO steward_sessions (id, steward_id, title, created_at, updated_at)
                 VALUES ('sess1', 'sai1', 'test', 1, 1)",
                [],
            )
            .unwrap();
        }

        update_soul(dbp.clone(), "sess1", "   ").await.unwrap();

        let db = Db::open(&dbp).unwrap();
        let role = roles::get(&db.0, STEWARD_MAIN_ROLE_ID).unwrap();
        assert!(role.system_prompt_override.is_none());
    }

    #[tokio::test]
    async fn update_soul_too_long_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_string_lossy().to_string();
        let db_conn = Db::open(&path_str).unwrap();
        migrations::run(&db_conn.0).unwrap();
        drop(db_conn);
        let dbp: Arc<str> = Arc::from(path_str);

        let long_content = "x".repeat(MAX_SOUL_LENGTH + 1);
        let result = update_soul(dbp, "sess1", &long_content).await;
        assert!(matches!(result, Err(SettingsError::SoulTooLong(_))));
    }

    #[tokio::test]
    async fn clear_messages_sets_cleared_at() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_string_lossy().to_string();
        let db_conn = Db::open(&path_str).unwrap();
        migrations::run(&db_conn.0).unwrap();
        drop(db_conn);
        let dbp: Arc<str> = Arc::from(path_str);

        {
            let db = Db::open(&dbp).unwrap();
            db.0.execute(
                "INSERT INTO sessions (id, title, created_at, updated_at, kind)
                 VALUES ('sess1', 'test', 1, 1, 'background')",
                [],
            )
            .unwrap();
            db.0.execute(
                "INSERT INTO steward_ai (id, ready, online_authorized, created_at)
                 VALUES ('sai1', 1, 0, 1)",
                [],
            )
            .unwrap();
            db.0.execute(
                "INSERT INTO steward_sessions (id, steward_id, title, created_at, updated_at)
                 VALUES ('sess1', 'sai1', 'test', 1, 1)",
                [],
            )
            .unwrap();
        }

        clear_messages(dbp.clone(), "sess1").await.unwrap();

        let db = Db::open(&dbp).unwrap();
        let ca: Option<i64> =
            db.0.query_row(
                "SELECT cleared_at FROM steward_sessions WHERE id = 'sess1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(ca.is_some(), "cleared_at should be set");
    }
}
