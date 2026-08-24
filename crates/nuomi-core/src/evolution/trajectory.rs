//! Trajectory aggregation: folds a session's append-only event log into a
//! structured summary usable as reflection input (AC12).

use crate::domain::EventRecord;

/// Structured digest of one session trajectory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrajectorySummary {
    pub session_id: String,
    /// Number of user turns (user-role `message` events).
    pub turns: usize,
    /// Number of `tool_call` events.
    pub tool_calls: usize,
    /// Contents of every `message` event, in seq order.
    pub outcomes: Vec<String>,
}

/// Aggregates raw [`EventRecord`]s (read via
/// `store::repos::events::list_by_aggregate`) into a [`TrajectorySummary`].
pub struct TrajectoryAggregator;

impl TrajectoryAggregator {
    pub fn aggregate(session_id: &str, events: &[EventRecord]) -> TrajectorySummary {
        let mut summary = TrajectorySummary {
            session_id: session_id.to_string(),
            turns: 0,
            tool_calls: 0,
            outcomes: Vec::new(),
        };
        for e in events {
            match e.kind.as_str() {
                "message" => {
                    if e.payload.get("role").and_then(|r| r.as_str()) == Some("user") {
                        summary.turns += 1;
                    }
                    if let Some(content) = e.payload.get("content").and_then(|c| c.as_str()) {
                        summary.outcomes.push(content.to_string());
                    }
                }
                "tool_call" => summary.tool_calls += 1,
                _ => {}
            }
        }
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;
    use crate::store::repos::events;
    use rusqlite::Connection;
    use serde_json::json;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    #[test]
    fn aggregates_two_session_fixtures() {
        let conn = db();
        // Session s1: two user turns, one tool call, assistant answer.
        events::append(
            &conn,
            "session",
            "s1",
            "message",
            &json!({"role":"user","content":"fix the bug"}),
            1,
        )
        .unwrap();
        events::append(
            &conn,
            "session",
            "s1",
            "thought",
            &json!({"text":"plan"}),
            2,
        )
        .unwrap();
        events::append(
            &conn,
            "session",
            "s1",
            "tool_call",
            &json!({"tool":"grep","arguments":{}}),
            3,
        )
        .unwrap();
        events::append(
            &conn,
            "session",
            "s1",
            "tool_result",
            &json!({"call_id":"c1","content":"line 42"}),
            4,
        )
        .unwrap();
        events::append(
            &conn,
            "session",
            "s1",
            "message",
            &json!({"role":"assistant","content":"bug fixed in line 42"}),
            5,
        )
        .unwrap();
        // Session s2: single turn, no tools.
        events::append(
            &conn,
            "session",
            "s2",
            "message",
            &json!({"role":"user","content":"hello"}),
            6,
        )
        .unwrap();
        events::append(
            &conn,
            "session",
            "s2",
            "message",
            &json!({"role":"assistant","content":"hi!"}),
            7,
        )
        .unwrap();

        let ev1 = events::list_by_aggregate(&conn, "session", "s1", None).unwrap();
        let t1 = TrajectoryAggregator::aggregate("s1", &ev1);
        assert_eq!(t1.session_id, "s1");
        assert_eq!(t1.turns, 1);
        assert_eq!(t1.tool_calls, 1);
        assert_eq!(
            t1.outcomes,
            vec![
                "fix the bug".to_string(),
                "bug fixed in line 42".to_string()
            ]
        );

        let ev2 = events::list_by_aggregate(&conn, "session", "s2", None).unwrap();
        let t2 = TrajectoryAggregator::aggregate("s2", &ev2);
        assert_eq!(t2.turns, 1);
        assert_eq!(t2.tool_calls, 0);
        assert_eq!(t2.outcomes.len(), 2);
    }

    #[test]
    fn empty_event_list_yields_empty_summary() {
        let summary = TrajectoryAggregator::aggregate("none", &[]);
        assert_eq!(summary.turns, 0);
        assert_eq!(summary.tool_calls, 0);
        assert!(summary.outcomes.is_empty());
    }
}
