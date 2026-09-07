//! Journal mirror repository: lands Harness Journal entries into the
//! `events` table so the *existing* `list_events` IPC can serve the Harness
//! Journal view without a new command. Two rows per entry:
//! - canonical aggregate `journal/<domain>` (the queryable audit record), and
//! - a session-scoped mirror under the synthetic `journal` session, because
//!   `list_events` validates the session and only reads that aggregate.

use rusqlite::{params, Connection};

use crate::store::StoreError;

use super::events;

/// Canonical aggregate type for journal entries.
pub const JOURNAL_AGGREGATE: &str = "journal";

/// Synthetic session id hosting the list_events-readable mirror. Must match
/// `evolution::journal::JOURNAL_SINK_SESSION_ID`.
pub const JOURNAL_SINK_SESSION_ID: &str = "journal";

const JOURNAL_SINK_TITLE: &str = "Harness Journal";

/// Ensures the synthetic sink session row exists (`list_events` validates it
/// before reading events).
pub fn ensure_sink_session(conn: &Connection, at: i64) -> Result<(), StoreError> {
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
        params![JOURNAL_SINK_SESSION_ID, JOURNAL_SINK_TITLE, at],
    )?;
    Ok(())
}

/// Mirrors one journal entry into the events table (canonical + session
/// aggregates). Append-only; mirrors are never rewritten.
pub fn append_mirror(
    conn: &Connection,
    domain: &str,
    kind: &str,
    payload: &serde_json::Value,
    created_at: i64,
) -> Result<(), StoreError> {
    ensure_sink_session(conn, created_at)?;
    events::append(conn, JOURNAL_AGGREGATE, domain, kind, payload, created_at)?;
    events::append(
        conn,
        "session",
        JOURNAL_SINK_SESSION_ID,
        kind,
        payload,
        created_at,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;
    use serde_json::json;

    #[test]
    fn mirror_creates_sink_session_and_two_event_rows() {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        append_mirror(
            &conn,
            "system_prompt",
            "journal.applied",
            &json!({ "seq": 1 }),
            42,
        )
        .unwrap();
        assert!(get_session(&conn, JOURNAL_SINK_SESSION_ID));
        assert_eq!(
            events::list_by_aggregate(&conn, JOURNAL_AGGREGATE, "system_prompt", None)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            events::list_by_aggregate(&conn, "session", JOURNAL_SINK_SESSION_ID, None)
                .unwrap()
                .len(),
            1
        );
        // Mirrors are idempotent per call: a second entry appends, never rewrites.
        append_mirror(
            &conn,
            "system_prompt",
            "journal.applied",
            &json!({ "seq": 2 }),
            43,
        )
        .unwrap();
        assert_eq!(
            events::list_by_aggregate(&conn, "session", JOURNAL_SINK_SESSION_ID, None)
                .unwrap()
                .len(),
            2
        );
    }

    fn get_session(conn: &Connection, id: &str) -> bool {
        conn.query_row("SELECT 1 FROM sessions WHERE id = ?1", params![id], |row| {
            row.get::<_, i64>(0)
        })
        .is_ok()
    }
}
