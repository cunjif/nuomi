//! Attachment repository — per-session file/image/paste metadata.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{Attachment, AttachmentKind};
use crate::store::StoreError;

pub fn insert(conn: &Connection, a: &Attachment) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO attachments (id, session_id, seq, kind, name, mime, rel_path, size_bytes, sha256, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            a.id,
            a.session_id,
            a.seq,
            a.kind.as_str(),
            a.name,
            a.mime,
            a.rel_path,
            a.size_bytes,
            a.sha256,
            a.created_at,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Attachment, StoreError> {
    conn.query_row(
        "SELECT id, session_id, seq, kind, name, mime, rel_path, size_bytes, sha256, created_at
         FROM attachments WHERE id = ?1",
        params![id],
        row_to_attachment,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "attachment",
        id: id.to_string(),
    })
}

pub fn list_by_session(conn: &Connection, session_id: &str) -> Result<Vec<Attachment>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, seq, kind, name, mime, rel_path, size_bytes, sha256, created_at
         FROM attachments WHERE session_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![session_id], row_to_attachment)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn delete(conn: &Connection, id: &str) -> Result<(), StoreError> {
    let n = conn.execute("DELETE FROM attachments WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "attachment",
            id: id.to_string(),
        });
    }
    Ok(())
}

pub fn update_seq(conn: &Connection, id: &str, seq: i64) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE attachments SET seq = ?2 WHERE id = ?1",
        params![id, seq],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "attachment",
            id: id.to_string(),
        });
    }
    Ok(())
}

fn row_to_attachment(row: &rusqlite::Row<'_>) -> rusqlite::Result<Attachment> {
    let kind_str: String = row.get(3)?;
    let kind = AttachmentKind::parse(&kind_str).unwrap_or(AttachmentKind::File);
    Ok(Attachment {
        id: row.get(0)?,
        session_id: row.get(1)?,
        seq: row.get(2)?,
        kind,
        name: row.get(4)?,
        mime: row.get(5)?,
        rel_path: row.get(6)?,
        size_bytes: row.get(7)?,
        sha256: row.get(8)?,
        created_at: row.get(9)?,
    })
}
