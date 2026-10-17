//! Steward event types + logger (K-Steward-8, T8-1 ~ T8-2).
//!
//! All steward events use `aggregate_type = "steward"` and are append-only
//! via `store::repos::events::append`. The original 17 event kinds match
//! design.md §2.3.2.4 exactly; the steward agent loop (steward_workbench_v2
//! design §2.1.3.1) adds `ToolCall`, `ToolResult`, `ContextPruned`, `Message`,
//! `ReplyCompleted` and `ConfigChangeCancelled`; the degrade paths (spec
//! 5.1.3-1b/3b) add `LlmFailed` and `SafetyRefused`.
//!
//! `steward.delta` is the one non-persistent steward topic: live streaming
//! notifications published on the EventBus only (never appended to the
//! events table) — see [`TOPIC_DELTA`].

use std::sync::Arc;

use rusqlite::Connection;

use crate::domain::now_ms;
use crate::store::repos::events;
use crate::store::StoreError;

/// Non-persistent live-streaming topic for steward reply deltas.
///
/// Published on the EventBus only — unlike every [`StewardEventKind`], it is
/// NEVER appended to the events table (append-only audit stays clean; the
/// authoritative final text lands as `steward.message` instead).
///
/// Payload shape: `{ "sessionId": String, "text": String, "seq": u64 }`
/// where `seq` is 1-based and monotonic within one reply turn.
pub const TOPIC_DELTA: &str = "steward.delta";

/// Steward event kinds (design.md §2.3.2.4 — 17 variants, plus 6 agent-loop
/// variants from steward_workbench_v2 design §2.1.3.1 / §2.2.2-A1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StewardEventKind {
    MessageReceived,
    IntentRecognized,
    ConfigProposed,
    ConfigConfirmed,
    ConfigRolledBack,
    CycleStarted,
    CyclePhaseChanged,
    CycleCompleted,
    CycleCancelled,
    CycleFailed,
    TaskDispatched,
    TaskCompleted,
    ArtifactSubmitted,
    ArtifactResolved,
    ArtifactMerged,
    CleanseCompleted,
    DevRoleBindingChanged,
    ToolCall,
    ToolResult,
    ContextPruned,
    Message,
    ReplyCompleted,
    ConfigChangeCancelled,
    LlmFailed,
    SafetyRefused,
}

impl StewardEventKind {
    /// Returns the string kind used in the `events` table (e.g. `"steward.message_received"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MessageReceived => "steward.message_received",
            Self::IntentRecognized => "steward.intent_recognized",
            Self::ConfigProposed => "steward.config_proposed",
            Self::ConfigConfirmed => "steward.config_confirmed",
            Self::ConfigRolledBack => "steward.config_rolled_back",
            Self::CycleStarted => "steward.cycle_started",
            Self::CyclePhaseChanged => "steward.cycle_phase_changed",
            Self::CycleCompleted => "steward.cycle_completed",
            Self::CycleCancelled => "steward.cycle_cancelled",
            Self::CycleFailed => "steward.cycle_failed",
            Self::TaskDispatched => "steward.task_dispatched",
            Self::TaskCompleted => "steward.task_completed",
            Self::ArtifactSubmitted => "steward.artifact_submitted",
            Self::ArtifactResolved => "steward.artifact_resolved",
            Self::ArtifactMerged => "steward.artifact_merged",
            Self::CleanseCompleted => "steward.cleanse_completed",
            Self::DevRoleBindingChanged => "steward.dev_role_binding_changed",
            Self::ToolCall => "steward.tool_call",
            Self::ToolResult => "steward.tool_result",
            Self::ContextPruned => "steward.context_pruned",
            Self::Message => "steward.message",
            Self::ReplyCompleted => "steward.reply_completed",
            Self::ConfigChangeCancelled => "steward.config_change_cancelled",
            Self::LlmFailed => "steward.llm_failed",
            Self::SafetyRefused => "steward.safety_refused",
        }
    }

    /// All kinds in declaration order.
    pub fn all() -> &'static [StewardEventKind] {
        &[
            Self::MessageReceived,
            Self::IntentRecognized,
            Self::ConfigProposed,
            Self::ConfigConfirmed,
            Self::ConfigRolledBack,
            Self::CycleStarted,
            Self::CyclePhaseChanged,
            Self::CycleCompleted,
            Self::CycleCancelled,
            Self::CycleFailed,
            Self::TaskDispatched,
            Self::TaskCompleted,
            Self::ArtifactSubmitted,
            Self::ArtifactResolved,
            Self::ArtifactMerged,
            Self::CleanseCompleted,
            Self::DevRoleBindingChanged,
            Self::ToolCall,
            Self::ToolResult,
            Self::ContextPruned,
            Self::Message,
            Self::ReplyCompleted,
            Self::ConfigChangeCancelled,
            Self::LlmFailed,
            Self::SafetyRefused,
        ]
    }
}

/// Appends a steward event to the `events` table.
///
/// `aggregate_type` is always `"steward"`; `aggregate_id` scopes the event
/// (e.g. session_id, cycle_id, artifact_id). The event is append-only —
/// the `events` table triggers reject UPDATE/DELETE.
pub fn log(
    conn: &Connection,
    aggregate_id: &str,
    kind: StewardEventKind,
    payload: &serde_json::Value,
) -> Result<i64, StoreError> {
    let record = events::append(
        conn,
        "steward",
        aggregate_id,
        kind.as_str(),
        payload,
        now_ms(),
    )?;
    Ok(record.id)
}

/// Best-effort async event publish (non-fatal on failure).
/// Opens its own DB connection — safe to call from async context.
pub async fn publish_event(
    db_path: std::sync::Arc<str>,
    aggregate_id: &str,
    kind: StewardEventKind,
    payload: &serde_json::Value,
) {
    let aggregate_id = aggregate_id.to_string();
    let payload = payload.clone();
    let _ = tokio::task::spawn_blocking(move || {
        let db = crate::store::Db::open(&db_path)?;
        log(&db.0, &aggregate_id, kind, &payload)?;
        Ok::<_, StoreError>(())
    })
    .await;
}

/// Best-effort live publish onto the EventBus — the non-persistent half of
/// the steward streaming channel (design §2.1 "管家流式事件通道": the
/// durable authority stays in the `events` table via [`publish_event`]).
///
/// Bus payloads MUST carry `sessionId` (camelCase) so the shell bridge
/// (`src-tauri/src/events.rs`) can route them to the per-session channel
/// `event://session/{id}`. Nothing is appended to the events table here.
pub fn publish_bus(
    bus: Option<&crate::harness::EventBus>,
    topic: &str,
    payload: serde_json::Value,
) {
    if let Some(bus) = bus {
        bus.publish(crate::harness::Event::new(topic, payload));
    }
}

// ============================================================ T8-8: Journal mirror

/// Mirrors key steward actions to the EvolutionJournal (fire-and-forget).
///
/// Only the three key actions specified in design.md are mirrored:
/// `cycle_started`, `phase_changed`, `artifact_merged`. Uses the existing
/// `evolution::journal::audit` helper — does not modify `EvolutionJournal`.
pub fn mirror_to_journal(
    journal: &Option<Arc<crate::evolution::journal::EvolutionJournal>>,
    kind: StewardEventKind,
    aggregate_id: &str,
    summary: &str,
    payload: serde_json::Value,
) {
    use crate::evolution::journal::{audit, JournalKind};

    let journal_kind = match kind {
        StewardEventKind::CycleStarted => JournalKind::ReflectionTriggered,
        StewardEventKind::CyclePhaseChanged => JournalKind::BaselineCaptured {
            digest: summary.into(),
        },
        StewardEventKind::ArtifactMerged => JournalKind::Applied {
            before_ref: None,
            after_ref: aggregate_id.into(),
        },
        _ => return, // only mirror the 3 key actions
    };

    audit(
        journal,
        "steward",
        journal_kind,
        "steward",
        summary.into(),
        vec![],
        payload,
    );
}

// ============================================================ T8-9: online authorization

/// Sets the online-learning authorization (shared with `evolution::research`).
///
/// Uses the same `MemoryService` and the same marker key, so the steward
/// and the evolution research scheduler share a single authorization state.
pub async fn set_online_authorization(
    mem: &crate::plugins::MemoryService,
    authorized: bool,
) -> Result<(), StoreError> {
    crate::evolution::research::set_online_authorized(mem, authorized).await
}

/// Reads the online-learning authorization (shared with `evolution::research`).
pub async fn online_authorized(mem: &crate::plugins::MemoryService) -> bool {
    crate::evolution::research::online_authorized(mem).await
}

/// Lists steward events, optionally filtered by `aggregate_id` and/or `kind_prefix`.
/// Results are ordered by `id ASC` (insertion order), limited to `limit` rows.
pub fn list(
    conn: &Connection,
    aggregate_id: Option<&str>,
    kind_prefix: Option<&str>,
    limit: u32,
) -> Result<Vec<crate::domain::EventRecord>, StoreError> {
    let mut sql = String::from(
        "SELECT id, aggregate_type, aggregate_id, kind, payload, seq, created_at \
         FROM events WHERE aggregate_type = 'steward'",
    );
    let mut params_vec: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(aid) = aggregate_id {
        sql.push_str(" AND aggregate_id = ?");
        params_vec.push(Box::new(aid.to_string()));
    }
    if let Some(prefix) = kind_prefix {
        sql.push_str(" AND kind LIKE ?");
        params_vec.push(Box::new(format!("{prefix}%")));
    }
    sql.push_str(" ORDER BY id ASC LIMIT ?");
    params_vec.push(Box::new(limit));

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        let payload: String = row.get(4)?;
        Ok(crate::domain::EventRecord {
            id: row.get(0)?,
            aggregate_type: row.get(1)?,
            aggregate_id: row.get(2)?,
            kind: row.get(3)?,
            payload: crate::store::json_col(4, &payload)?,
            seq: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;
    use serde_json::json;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    #[test]
    fn all_kinds_have_distinct_strings() {
        let kinds = StewardEventKind::all();
        assert_eq!(kinds.len(), 25);
        let strs: Vec<&str> = kinds.iter().map(|k| k.as_str()).collect();
        let unique: std::collections::HashSet<&str> = strs.iter().copied().collect();
        assert_eq!(unique.len(), 25, "all 25 kind strings must be distinct");
    }

    #[test]
    fn delta_topic_is_not_a_persistent_kind() {
        // `steward.delta` is the non-persistent streaming topic: it must
        // never appear in the append-only kinds, and no persistent kind may
        // collide with it.
        assert_eq!(TOPIC_DELTA, "steward.delta");
        for kind in StewardEventKind::all() {
            assert_ne!(
                kind.as_str(),
                TOPIC_DELTA,
                "steward.delta must stay out of the persistent kinds"
            );
        }
    }

    #[test]
    fn log_appends_event() {
        let conn = db();
        let id1 = log(
            &conn,
            "s1",
            StewardEventKind::MessageReceived,
            &json!({"text": "hi"}),
        )
        .unwrap();
        let id2 = log(
            &conn,
            "s1",
            StewardEventKind::IntentRecognized,
            &json!({"intent": "cfg"}),
        )
        .unwrap();
        assert!(id2 > id1);
    }

    #[test]
    fn list_filters_by_aggregate_id() {
        let conn = db();
        log(&conn, "s1", StewardEventKind::MessageReceived, &json!({})).unwrap();
        log(&conn, "s2", StewardEventKind::MessageReceived, &json!({})).unwrap();
        let evts = list(&conn, Some("s1"), None, 100).unwrap();
        assert_eq!(evts.len(), 1);
        assert_eq!(evts[0].aggregate_id, "s1");
    }

    #[test]
    fn list_filters_by_kind_prefix() {
        let conn = db();
        log(&conn, "c1", StewardEventKind::CycleStarted, &json!({})).unwrap();
        log(&conn, "c1", StewardEventKind::CycleCompleted, &json!({})).unwrap();
        log(&conn, "c1", StewardEventKind::ArtifactSubmitted, &json!({})).unwrap();
        let cycle_evts = list(&conn, None, Some("steward.cycle_"), 100).unwrap();
        assert_eq!(cycle_evts.len(), 2);
        for e in &cycle_evts {
            assert!(e.kind.starts_with("steward.cycle_"));
        }
    }

    #[test]
    fn list_respects_limit() {
        let conn = db();
        for _ in 0..5 {
            log(&conn, "s1", StewardEventKind::MessageReceived, &json!({})).unwrap();
        }
        let evts = list(&conn, Some("s1"), None, 3).unwrap();
        assert_eq!(evts.len(), 3);
    }

    #[test]
    fn events_are_append_only() {
        let conn = db();
        log(&conn, "s1", StewardEventKind::MessageReceived, &json!({})).unwrap();
        assert!(conn.execute("UPDATE events SET kind = 'x'", []).is_err());
        assert!(conn.execute("DELETE FROM events", []).is_err());
    }

    #[test]
    fn all_kinds_prefixed_with_steward() {
        for kind in StewardEventKind::all() {
            assert!(
                kind.as_str().starts_with("steward."),
                "kind {} should start with 'steward.'",
                kind.as_str()
            );
        }
    }

    #[test]
    fn publish_event_async_works() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("test.db");
            let path_str = path.to_string_lossy().to_string();
            let db_conn = crate::store::Db::open(&path_str).unwrap();
            migrations::run(&db_conn.0).unwrap();
            drop(db_conn);
            let dbp: Arc<str> = Arc::from(path_str);

            publish_event(
                dbp.clone(),
                "s1",
                StewardEventKind::MessageReceived,
                &json!({"text": "hello"}),
            )
            .await;

            let db = crate::store::Db::open(&dbp).unwrap();
            let evts = list(&db.0, Some("s1"), None, 100).unwrap();
            assert_eq!(evts.len(), 1);
            assert_eq!(evts[0].kind, "steward.message_received");
        });
    }

    #[test]
    fn steward_events_do_not_pollute_other_aggregates() {
        let conn = db();
        // steward event
        log(&conn, "s1", StewardEventKind::MessageReceived, &json!({})).unwrap();
        // non-steward event (simulating existing evolution/journal usage)
        events::append(&conn, "session", "s1", "message", &json!({}), now_ms()).unwrap();

        // steward list only returns steward events
        let steward_evts = list(&conn, None, None, 100).unwrap();
        assert_eq!(steward_evts.len(), 1);
        assert_eq!(steward_evts[0].aggregate_type, "steward");

        // non-steward events are still accessible via events::list_by_aggregate
        let other_evts = events::list_by_aggregate(&conn, "session", "s1", None).unwrap();
        assert_eq!(other_evts.len(), 1);
        assert_eq!(other_evts[0].aggregate_type, "session");
    }

    #[test]
    fn all_kinds_can_be_logged() {
        let conn = db();
        for kind in StewardEventKind::all() {
            log(&conn, "test", *kind, &json!({"kind": kind.as_str()})).unwrap();
        }
        let evts = list(&conn, Some("test"), None, 100).unwrap();
        assert_eq!(evts.len(), 25);
    }

    // T8-13: zero-intrusion verification

    #[test]
    fn zero_intrusion_events_table_unchanged() {
        let conn = db();
        // Verify the events table still accepts non-steward aggregate types
        // (existing evolution/journal/session code must not be affected)
        let e1 = events::append(&conn, "session", "s1", "message", &json!({}), 1).unwrap();
        let e2 = events::append(&conn, "journal", "j1", "applied", &json!({}), 2).unwrap();
        assert_eq!(e1.aggregate_type, "session");
        assert_eq!(e2.aggregate_type, "journal");

        // Steward events coexist without interference
        log(&conn, "st1", StewardEventKind::CycleStarted, &json!({})).unwrap();
        let session_evts = events::list_by_aggregate(&conn, "session", "s1", None).unwrap();
        let journal_evts = events::list_by_aggregate(&conn, "journal", "j1", None).unwrap();
        let steward_evts = list(&conn, None, None, 100).unwrap();
        assert_eq!(session_evts.len(), 1);
        assert_eq!(journal_evts.len(), 1);
        assert_eq!(steward_evts.len(), 1);
    }
}
