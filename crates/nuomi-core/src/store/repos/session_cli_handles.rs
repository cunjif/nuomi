//! Per (session, role_agent) CLI Agent session handles (ADR 0012 D4).
//! Stores the CLI Agent's own session id for resume — different Role Agents
//! each hold an independent handle, even if bound to the same CLI Agent.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::SessionCliHandle;
use crate::store::StoreError;

pub fn get(
    conn: &Connection,
    session_id: &str,
    role_agent_id: &str,
) -> Result<Option<SessionCliHandle>, StoreError> {
    conn.query_row(
        "SELECT session_id, role_agent_id, agent_profile_id, cli_session_id, updated_at
         FROM session_cli_handles WHERE session_id = ?1 AND role_agent_id = ?2",
        params![session_id, role_agent_id],
        row_to_handle,
    )
    .optional()
    .map_err(StoreError::from)
}

pub fn upsert(conn: &Connection, h: &SessionCliHandle) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO session_cli_handles
         (session_id, role_agent_id, agent_profile_id, cli_session_id, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(session_id, role_agent_id) DO UPDATE SET
           agent_profile_id = excluded.agent_profile_id,
           cli_session_id = excluded.cli_session_id,
           updated_at = excluded.updated_at",
        params![
            h.session_id,
            h.role_agent_id,
            h.agent_profile_id,
            h.cli_session_id,
            h.updated_at
        ],
    )?;
    Ok(())
}

pub fn clear(conn: &Connection, session_id: &str, role_agent_id: &str) -> Result<bool, StoreError> {
    let n = conn.execute(
        "DELETE FROM session_cli_handles WHERE session_id = ?1 AND role_agent_id = ?2",
        params![session_id, role_agent_id],
    )?;
    Ok(n > 0)
}

fn row_to_handle(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionCliHandle> {
    Ok(SessionCliHandle {
        session_id: row.get(0)?,
        role_agent_id: row.get(1)?,
        agent_profile_id: row.get(2)?,
        cli_session_id: row.get(3)?,
        updated_at: row.get(4)?,
    })
}
