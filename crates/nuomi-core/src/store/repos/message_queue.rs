//! Message queue repository (ADR 0015).

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{MessageQueueEntry, QueueStatus};
use crate::store::StoreError;

/// Enqueues a message for `session_id`. Returns the inserted entry.
pub fn enqueue(
    conn: &Connection,
    session_id: &str,
    text: &str,
) -> Result<MessageQueueEntry, StoreError> {
    let now = crate::domain::now_ms();
    let id = crate::domain::new_id();
    let seq = next_seq(conn, session_id)?;
    conn.execute(
        "INSERT INTO message_queue (id, session_id, text, status, seq, created_at)
         VALUES (?1, ?2, ?3, 'queued', ?4, ?5)",
        params![id, session_id, text, seq, now],
    )?;
    Ok(MessageQueueEntry {
        id,
        session_id: session_id.to_string(),
        text: text.to_string(),
        status: QueueStatus::Queued,
        seq,
        created_at: now,
    })
}

fn next_seq(conn: &Connection, session_id: &str) -> Result<i64, StoreError> {
    let max: Option<i64> = conn
        .query_row(
            "SELECT MAX(seq) FROM message_queue WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(max.unwrap_or(0) + 1)
}

/// Lists all `queued` messages for `session_id`, ordered by seq.
pub fn list_queued(conn: &Connection, session_id: &str) -> Result<Vec<MessageQueueEntry>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, text, status, seq, created_at
         FROM message_queue WHERE session_id = ?1 AND status = 'queued'
         ORDER BY seq ASC",
    )?;
    let rows = stmt.query_map(params![session_id], row_to_entry)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Dequeues the oldest `queued` message for `session_id`, marking it
/// `processing`. Returns `None` when the queue is empty.
///
/// The SELECT + UPDATE are wrapped in a transaction with `BEGIN IMMEDIATE`
/// so concurrent dequeues on separate connections serialize at the SQLite
/// level and cannot both pick the same row.
pub fn dequeue(conn: &Connection, session_id: &str) -> Result<Option<MessageQueueEntry>, StoreError> {
    conn.execute("BEGIN IMMEDIATE", [])?;
    let result = (|| -> Result<Option<MessageQueueEntry>, StoreError> {
        let entry: Option<(String, String, String, i64, i64)> = conn
            .query_row(
                "SELECT id, session_id, text, seq, created_at
                 FROM message_queue WHERE session_id = ?1 AND status = 'queued'
                 ORDER BY seq ASC LIMIT 1",
                params![session_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        match entry {
            Some((id, sid, text, seq, created_at)) => {
                conn.execute(
                    "UPDATE message_queue SET status = 'processing' WHERE id = ?1",
                    params![id],
                )?;
                Ok(Some(MessageQueueEntry {
                    id,
                    session_id: sid,
                    text,
                    status: QueueStatus::Processing,
                    seq,
                    created_at,
                }))
            }
            None => Ok(None),
        }
    })();
    // Always commit (or rollback on error) to release the write lock.
    match result {
        Ok(v) => {
            conn.execute("COMMIT", [])?;
            Ok(v)
        }
        Err(e) => {
            let _ = conn.execute("ROLLBACK", []);
            Err(e)
        }
    }
}

/// Marks a queue entry as `done` (turn completed successfully).
pub fn mark_done(conn: &Connection, id: &str) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE message_queue SET status = 'done' WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Marks a queue entry as `failed` (turn errored). The entry is preserved
/// for diagnostics; it is no longer `queued` so the drainer will not retry
/// it automatically.
pub fn mark_failed(conn: &Connection, id: &str) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE message_queue SET status = 'failed' WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Cancels (deletes) a single queued entry. Only `queued` entries can be
/// cancelled — `processing` entries are already in flight.
pub fn cancel(conn: &Connection, id: &str) -> Result<(), StoreError> {
    let n = conn.execute(
        "DELETE FROM message_queue WHERE id = ?1 AND status = 'queued'",
        params![id],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "message_queue",
            id: id.to_string(),
        });
    }
    Ok(())
}

/// Clears all `queued` entries for `session_id`. Returns the count removed.
pub fn clear_queued(conn: &Connection, session_id: &str) -> Result<usize, StoreError> {
    let n = conn.execute(
        "DELETE FROM message_queue WHERE session_id = ?1 AND status = 'queued'",
        params![session_id],
    )?;
    Ok(n)
}

/// Counts remaining `queued` entries for `session_id`.
pub fn count_queued(conn: &Connection, session_id: &str) -> Result<i64, StoreError> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM message_queue WHERE session_id = ?1 AND status = 'queued'",
        params![session_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageQueueEntry> {
    let status_str: String = row.get(3)?;
    let status = match status_str.as_str() {
        "processing" => QueueStatus::Processing,
        "done" => QueueStatus::Done,
        "failed" => QueueStatus::Failed,
        _ => QueueStatus::Queued,
    };
    Ok(MessageQueueEntry {
        id: row.get(0)?,
        session_id: row.get(1)?,
        text: row.get(2)?,
        status,
        seq: row.get(4)?,
        created_at: row.get(5)?,
    })
}
