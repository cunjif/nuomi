//! Event rollback marker repository.
//!
//! Rollback is a soft-hide: events remain in the append-only `events` table,
//! and a row here marks a `[from_seq, to_seq]` range as rolled back.
//! `events::list_by_aggregate` filters rows that fall within any rollback range.

use rusqlite::{params, Connection};

use crate::domain::EventRollback;
use crate::store::StoreError;

/// Inserts a rollback marker for `[from_seq, to_seq]` on the given aggregate.
pub fn insert(
    conn: &Connection,
    aggregate_type: &str,
    aggregate_id: &str,
    from_seq: i64,
    to_seq: i64,
    rolled_back_at: i64,
    reason: Option<&str>,
) -> Result<EventRollback, StoreError> {
    conn.execute(
        "INSERT INTO event_rollbacks
            (aggregate_type, aggregate_id, from_seq, to_seq, rolled_back_at, reason)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            aggregate_type,
            aggregate_id,
            from_seq,
            to_seq,
            rolled_back_at,
            reason
        ],
    )?;
    Ok(EventRollback {
        id: conn.last_insert_rowid(),
        aggregate_type: aggregate_type.to_string(),
        aggregate_id: aggregate_id.to_string(),
        from_seq,
        to_seq,
        rolled_back_at,
        reason: reason.map(|s| s.to_string()),
    })
}

/// Lists all rollback markers for an aggregate, ordered by `rolled_back_at`.
pub fn list(
    conn: &Connection,
    aggregate_type: &str,
    aggregate_id: &str,
) -> Result<Vec<EventRollback>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, aggregate_type, aggregate_id, from_seq, to_seq, rolled_back_at, reason
         FROM event_rollbacks
         WHERE aggregate_type = ?1 AND aggregate_id = ?2
         ORDER BY rolled_back_at ASC",
    )?;
    let rows = stmt.query_map(params![aggregate_type, aggregate_id], row_to_rollback)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Returns true if `seq` falls within any rollback range for the aggregate.
pub fn exists_for_seq(
    conn: &Connection,
    aggregate_type: &str,
    aggregate_id: &str,
    seq: i64,
) -> Result<bool, StoreError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM event_rollbacks
         WHERE aggregate_type = ?1 AND aggregate_id = ?2 AND ?3 BETWEEN from_seq AND to_seq",
        params![aggregate_type, aggregate_id, seq],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

fn row_to_rollback(row: &rusqlite::Row<'_>) -> rusqlite::Result<EventRollback> {
    Ok(EventRollback {
        id: row.get(0)?,
        aggregate_type: row.get(1)?,
        aggregate_id: row.get(2)?,
        from_seq: row.get(3)?,
        to_seq: row.get(4)?,
        rolled_back_at: row.get(5)?,
        reason: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;
    use crate::store::repos::events;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    #[test]
    fn insert_and_list_roundtrip() {
        let conn = db();
        let r1 = insert(&conn, "session", "s1", 3, 5, 100, Some("user rollback")).unwrap();
        let r2 = insert(&conn, "session", "s1", 8, 10, 200, None).unwrap();
        let list = list(&conn, "session", "s1").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, r1.id);
        assert_eq!(list[0].from_seq, 3);
        assert_eq!(list[0].to_seq, 5);
        assert_eq!(list[0].reason.as_deref(), Some("user rollback"));
        assert_eq!(list[1].id, r2.id);
        assert_eq!(list[1].reason, None);
    }

    #[test]
    fn exists_for_seq_checks_inclusive_range() {
        let conn = db();
        insert(&conn, "session", "s1", 3, 5, 100, None).unwrap();
        assert!(exists_for_seq(&conn, "session", "s1", 3).unwrap());
        assert!(exists_for_seq(&conn, "session", "s1", 4).unwrap());
        assert!(exists_for_seq(&conn, "session", "s1", 5).unwrap());
        assert!(!exists_for_seq(&conn, "session", "s1", 2).unwrap());
        assert!(!exists_for_seq(&conn, "session", "s1", 6).unwrap());
    }

    #[test]
    fn list_by_aggregate_filters_rolled_back_events() {
        let conn = db();
        for i in 1..=5 {
            events::append(
                &conn,
                "session",
                "s1",
                "message",
                &serde_json::json!({ "i": i }),
                i,
            )
            .unwrap();
        }
        // Before rollback: all 5 events visible
        let before = events::list_by_aggregate(&conn, "session", "s1", None).unwrap();
        assert_eq!(before.len(), 5);

        // Roll back seq 3..=5
        insert(&conn, "session", "s1", 3, 5, 100, None).unwrap();

        // After rollback: only seq 1-2 visible
        let after = events::list_by_aggregate(&conn, "session", "s1", None).unwrap();
        assert_eq!(after.len(), 2);
        assert_eq!(after[0].seq, 1);
        assert_eq!(after[1].seq, 2);
    }

    #[test]
    fn no_rollback_records_means_no_filtering() {
        let conn = db();
        for i in 1..=3 {
            events::append(
                &conn,
                "session",
                "s1",
                "message",
                &serde_json::json!({ "i": i }),
                i,
            )
            .unwrap();
        }
        let all = events::list_by_aggregate(&conn, "session", "s1", None).unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn rollback_is_isolated_per_aggregate() {
        let conn = db();
        events::append(&conn, "session", "s1", "message", &serde_json::json!({}), 1).unwrap();
        events::append(&conn, "session", "s1", "message", &serde_json::json!({}), 2).unwrap();
        events::append(&conn, "session", "s2", "message", &serde_json::json!({}), 1).unwrap();
        events::append(&conn, "session", "s2", "message", &serde_json::json!({}), 2).unwrap();

        insert(&conn, "session", "s1", 2, 2, 100, None).unwrap();

        let s1 = events::list_by_aggregate(&conn, "session", "s1", None).unwrap();
        let s2 = events::list_by_aggregate(&conn, "session", "s2", None).unwrap();
        assert_eq!(s1.len(), 1);
        assert_eq!(s2.len(), 2);
    }
}
