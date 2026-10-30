//! ADR-0002 event bridge: partitions kernel bus events into Tauri channels.
//!
//! Pure logic lives here so it is testable without a Tauri runtime;
//! the actual `emit` wiring happens in `lib.rs`.

use nuomi_core::harness::Event;
use serde::Serialize;
use serde_json::Value;

/// Wire shape of every event pushed to the frontend (ipc-contract rules).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DomainEvent {
    /// Discriminant, e.g. `session.delta`, `run.state_changed`, `approval_requested`.
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Monotonic within the target channel's aggregate (gap recovery).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<i64>,
    pub payload: Value,
}

/// Session-scoped high-frequency topics (ADR-0002 namespace 1).
///
/// `steward.*` events are scoped to the steward session carried in the
/// payload's `sessionId` (streaming deltas, reply lifecycle) and follow the
/// same per-session channel pattern as `session.*` (steward_workbench_v2
/// design §2.2.2 table: "对齐 ADR-0002 session channel 模式").
fn is_session_topic(topic: &str) -> bool {
    topic.starts_with("session.")
        || topic.starts_with("tool.")
        || topic.starts_with("hook.")
        || topic.starts_with("steward.")
}

/// Global structured topics (ADR-0002 namespace 2).
fn is_domain_topic(topic: &str) -> bool {
    topic.starts_with("task.")
        || topic.starts_with("run.")
        || topic.starts_with("approval.")
        || topic.starts_with("proposal.")
        || topic.starts_with("schedule.")
        || topic.starts_with("team.")
        || topic.starts_with("change.")
        || topic.starts_with("conversation.")
        || topic.starts_with("sandbox.")
        || topic.starts_with("evolution.")
}

fn extract(event: &Event) -> (Option<String>, Option<String>, Option<String>, Option<i64>) {
    let p = &event.payload;
    (
        p.get("taskId").and_then(Value::as_str).map(str::to_string),
        p.get("runId").and_then(Value::as_str).map(str::to_string),
        p.get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_string),
        p.get("seq").and_then(Value::as_i64),
    )
}

/// Resolves the target channels and wire payloads for one kernel event.
/// Returns `(channel, DomainEvent)` pairs (usually 0 or 1).
pub fn partition_event(event: &Event) -> Vec<(String, DomainEvent)> {
    let (task_id, run_id, session_id, seq) = extract(event);
    let wire = DomainEvent {
        r#type: event.topic.clone(),
        task_id,
        run_id,
        session_id: session_id.clone(),
        seq,
        payload: event.payload.clone(),
    };
    if let Some(sid) = session_id {
        if is_session_topic(&event.topic) {
            return vec![(format!("event://session/{sid}"), wire)];
        }
    }
    if is_domain_topic(&event.topic) {
        return vec![("event://domain".to_string(), wire)];
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn table_routes_topics_to_channels() {
        let cases = [
            ("session.delta", Some("s1"), "event://session/s1"),
            ("tool.call", Some("s1"), "event://session/s1"),
            ("hook.pre_tool_call", Some("s2"), "event://session/s2"),
            ("steward.delta", Some("s1"), "event://session/s1"),
            ("session.cancelled", Some("s1"), "event://session/s1"),
            ("task.status_changed", None, "event://domain"),
            ("run.state_changed", None, "event://domain"),
            ("approval.requested", None, "event://domain"),
            // P2-F 组 2: proposal lifecycle reaches the approvals inbox.
            ("proposal.created", None, "event://domain"),
            ("proposal.resolved", None, "event://domain"),
            ("schedule.triggered", None, "event://domain"),
            ("team.formed", None, "event://domain"),
            // sandbox.<task_id> progress pushes (ADR-0022 §4)
            ("sandbox.t-123", None, "event://domain"),
        ];
        for (topic, session, expected_channel) in cases {
            let payload = match session {
                Some(s) => json!({ "sessionId": s, "seq": 7 }),
                None => json!({ "taskId": "t1", "runId": "r1" }),
            };
            let ev = Event::new(topic, payload);
            let routed = partition_event(&ev);
            assert_eq!(routed.len(), 1, "{topic} must route");
            assert_eq!(routed[0].0, expected_channel);
            assert_eq!(routed[0].1.r#type, topic);
        }
    }

    #[test]
    fn unknown_and_unbound_events_are_dropped() {
        assert!(partition_event(&Event::new("misc.noise", json!({}))).is_empty());
        // session-topic without sessionId cannot be routed safely
        assert!(partition_event(&Event::new("tool.call", json!({}))).is_empty());
        // steward topics without sessionId cannot be routed either
        assert!(partition_event(&Event::new("steward.delta", json!({}))).is_empty());
    }

    #[test]
    fn steward_delta_carries_stream_payload() {
        let ev = Event::new(
            "steward.delta",
            json!({ "sessionId": "st9", "text": "你", "seq": 3 }),
        );
        let routed = partition_event(&ev);
        assert_eq!(routed.len(), 1);
        let (channel, wire) = &routed[0];
        assert_eq!(channel, "event://session/st9");
        assert_eq!(wire.r#type, "steward.delta");
        assert_eq!(wire.session_id.as_deref(), Some("st9"));
        assert_eq!(wire.seq, Some(3));
        assert_eq!(
            wire.payload.get("text").and_then(|v| v.as_str()),
            Some("你")
        );
    }

    #[test]
    fn wire_fields_survive_partition() {
        let ev = Event::new(
            "run.state_changed",
            json!({ "taskId": "t", "runId": "r", "seq": 3 }),
        );
        let (_, wire) = &partition_event(&ev)[0];
        assert_eq!(wire.task_id.as_deref(), Some("t"));
        assert_eq!(wire.run_id.as_deref(), Some("r"));
        assert_eq!(wire.seq, Some(3));
    }
}
