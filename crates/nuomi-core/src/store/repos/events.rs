//! Append-only event repository.

use rusqlite::{params, Connection};

use crate::domain::EventRecord;
use crate::store::StoreError;

/// Appends an event, computing the next monotonic `seq` within the aggregate.
/// The `events` table rejects UPDATE/DELETE via triggers.
pub fn append(
    conn: &Connection,
    aggregate_type: &str,
    aggregate_id: &str,
    kind: &str,
    payload: &serde_json::Value,
    created_at: i64,
) -> Result<EventRecord, StoreError> {
    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM events WHERE aggregate_type = ?1 AND aggregate_id = ?2",
        params![aggregate_type, aggregate_id],
        |r| r.get(0),
    )?;
    let payload_json = serde_json::to_string(payload)?;
    conn.execute(
        "INSERT INTO events (aggregate_type, aggregate_id, kind, payload, seq, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            aggregate_type,
            aggregate_id,
            kind,
            payload_json,
            seq,
            created_at
        ],
    )?;
    Ok(EventRecord {
        id: conn.last_insert_rowid(),
        aggregate_type: aggregate_type.to_string(),
        aggregate_id: aggregate_id.to_string(),
        kind: kind.to_string(),
        payload: payload.clone(),
        seq,
        created_at,
    })
}

/// Lists events of an aggregate in seq order; `after_seq` enables gap recovery.
pub fn list_by_aggregate(
    conn: &Connection,
    aggregate_type: &str,
    aggregate_id: &str,
    after_seq: Option<i64>,
) -> Result<Vec<EventRecord>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, aggregate_type, aggregate_id, kind, payload, seq, created_at
         FROM events
         WHERE aggregate_type = ?1 AND aggregate_id = ?2 AND seq > ?3
         ORDER BY seq ASC",
    )?;
    let rows = stmt.query_map(
        params![aggregate_type, aggregate_id, after_seq.unwrap_or(0)],
        row_to_event,
    )?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<EventRecord> {
    let payload: String = row.get(4)?;
    Ok(EventRecord {
        id: row.get(0)?,
        aggregate_type: row.get(1)?,
        aggregate_id: row.get(2)?,
        kind: row.get(3)?,
        payload: crate::store::json_col(4, &payload)?,
        seq: row.get(5)?,
        created_at: row.get(6)?,
    })
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
    fn seq_is_monotonic_per_aggregate() {
        let conn = db();
        let e1 = append(&conn, "session", "s1", "message", &json!({}), 1).unwrap();
        let e2 = append(&conn, "session", "s1", "thought", &json!({}), 2).unwrap();
        assert_eq!((e1.seq, e2.seq), (1, 2));
        // different aggregate restarts at 1
        let other = append(&conn, "session", "s2", "message", &json!({}), 3).unwrap();
        assert_eq!(other.seq, 1);
    }

    #[test]
    fn update_and_delete_are_rejected() {
        let conn = db();
        append(&conn, "session", "s1", "message", &json!({}), 1).unwrap();
        assert!(conn.execute("UPDATE events SET kind = 'x'", []).is_err());
        assert!(conn.execute("DELETE FROM events", []).is_err());
    }

    #[test]
    fn after_seq_enables_gap_recovery() {
        let conn = db();
        for i in 1..=3 {
            append(&conn, "session", "s1", "message", &json!({ "i": i }), i).unwrap();
        }
        let rest = list_by_aggregate(&conn, "session", "s1", Some(1)).unwrap();
        assert_eq!(rest.len(), 2);
        assert_eq!(rest[0].payload["i"], 2);
    }
}
