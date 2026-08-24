//! WhiteBoard repository: append-only shared blackboard for group chats.

use rusqlite::{params, Connection};

use crate::domain::WhiteBoardNote;
use crate::store::StoreError;

/// Appends a note; seq is monotonic within the session.
pub fn append(conn: &Connection, note: &WhiteBoardNote) -> Result<(), StoreError> {
    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM whiteboard_notes WHERE session_id = ?1",
        params![note.session_id],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO whiteboard_notes (id, session_id, author_role_id, note_type, body, refs_json, seq, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            note.id,
            note.session_id,
            note.author_role_id,
            note.note_type,
            note.body,
            serde_json::to_string(&note.refs)?,
            seq,
            note.created_at
        ],
    )?;
    Ok(())
}

pub fn list_by_session(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<WhiteBoardNote>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, author_role_id, note_type, body, refs_json, seq, created_at
         FROM whiteboard_notes WHERE session_id = ?1 ORDER BY seq ASC",
    )?;
    let rows = stmt.query_map(params![session_id], row_note)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn row_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<WhiteBoardNote> {
    let refs: String = row.get(5)?;
    Ok(WhiteBoardNote {
        id: row.get(0)?,
        session_id: row.get(1)?,
        author_role_id: row.get(2)?,
        note_type: row.get(3)?,
        body: row.get(4)?,
        refs: crate::store::json_col(0, &refs)?,
        seq: row.get(6)?,
        created_at: row.get(7)?,
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
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES ('s1','','1','1')",
            [],
        )
        .unwrap();
        conn
    }

    fn note(id: &str, body: &str, at: i64) -> WhiteBoardNote {
        WhiteBoardNote {
            id: id.into(),
            session_id: "s1".into(),
            author_role_id: None,
            note_type: "finding".into(),
            body: body.into(),
            refs: json!({}),
            seq: 0, // assigned by repo
            created_at: at,
        }
    }

    #[test]
    fn appends_with_monotonic_seq_and_lists_in_order() {
        let conn = db();
        append(&conn, &note("n1", "first", 1)).unwrap();
        append(&conn, &note("n2", "second", 2)).unwrap();
        let notes = list_by_session(&conn, "s1").unwrap();
        assert_eq!(notes.iter().map(|n| n.seq).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(notes[1].body, "second");
    }

    #[test]
    fn update_and_delete_are_rejected() {
        let conn = db();
        append(&conn, &note("n1", "x", 1)).unwrap();
        assert!(conn
            .execute("UPDATE whiteboard_notes SET body='y'", [])
            .is_err());
        assert!(conn.execute("DELETE FROM whiteboard_notes", []).is_err());
    }
}
