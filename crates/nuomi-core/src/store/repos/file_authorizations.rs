//! File-authorization repository (`file_authorizations` table, migration 0040).
//!
//! Rows track write/delete authorization requests raised by RoleAgents
//! (chat_rich_bind spec 6.2). State transitions are monotonic toward terminal
//! states: `transition_with_event` appends the mirrored `authorization.*`
//! event and updates the state row inside ONE transaction, so callers only
//! perform external side effects after commit (event-sourcing iron rule).
//! `waiting_notified` is a derived UI state (pending + `timeout_notified`
//! event), never a row state.

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::store::repos::events;
use crate::store::StoreError;

const ENTITY: &str = "file_authorization";

/// The file operation an authorization covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationOp {
    Write,
    Delete,
}

impl AuthorizationOp {
    pub const fn as_str(self) -> &'static str {
        match self {
            AuthorizationOp::Write => "write",
            AuthorizationOp::Delete => "delete",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "write" => Some(AuthorizationOp::Write),
            "delete" => Some(AuthorizationOp::Delete),
            _ => None,
        }
    }
}

/// Authorization lifecycle state (row states only — see module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorizationState {
    Pending,
    GrantedOnce,
    GrantedSession,
    Denied,
    TimeoutDefault,
    Voided,
}

impl AuthorizationState {
    pub const fn as_str(self) -> &'static str {
        match self {
            AuthorizationState::Pending => "pending",
            AuthorizationState::GrantedOnce => "granted_once",
            AuthorizationState::GrantedSession => "granted_session",
            AuthorizationState::Denied => "denied",
            AuthorizationState::TimeoutDefault => "timeout_default",
            AuthorizationState::Voided => "voided",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(AuthorizationState::Pending),
            "granted_once" => Some(AuthorizationState::GrantedOnce),
            "granted_session" => Some(AuthorizationState::GrantedSession),
            "denied" => Some(AuthorizationState::Denied),
            "timeout_default" => Some(AuthorizationState::TimeoutDefault),
            "voided" => Some(AuthorizationState::Voided),
            _ => None,
        }
    }

    /// Terminal states admit no further transitions.
    pub const fn is_terminal(self) -> bool {
        !matches!(self, AuthorizationState::Pending)
    }

    /// Whether `from -> to` is a legal (monotonic toward terminal) move.
    pub const fn can_transition(self, to: AuthorizationState) -> bool {
        match self {
            AuthorizationState::Pending => !matches!(to, AuthorizationState::Pending),
            _ => false,
        }
    }
}

/// A `file_authorizations` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileAuthorizationEntry {
    pub id: String,
    pub session_id: String,
    pub role_id: String,
    pub op: AuthorizationOp,
    pub target_path: String,
    pub state: AuthorizationState,
    pub requested_at: i64,
    pub expires_at: i64,
    pub resolved_at: Option<i64>,
    pub resolution: Option<String>,
}

/// Inserts a pending authorization (uuid-v7 id, `expires_at = now + timeout_ms`).
pub fn insert_pending(
    conn: &Connection,
    session_id: &str,
    role_id: &str,
    op: AuthorizationOp,
    target_path: &str,
    timeout_ms: i64,
) -> Result<FileAuthorizationEntry, StoreError> {
    let now = crate::domain::now_ms();
    let entry = FileAuthorizationEntry {
        id: Uuid::now_v7().to_string(),
        session_id: session_id.to_string(),
        role_id: role_id.to_string(),
        op,
        target_path: target_path.to_string(),
        state: AuthorizationState::Pending,
        requested_at: now,
        expires_at: now + timeout_ms,
        resolved_at: None,
        resolution: None,
    };
    conn.execute(
        "INSERT INTO file_authorizations (id, session_id, role_id, op, target_path, state, requested_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            entry.id,
            entry.session_id,
            entry.role_id,
            entry.op.as_str(),
            entry.target_path,
            entry.state.as_str(),
            entry.requested_at,
            entry.expires_at
        ],
    )?;
    Ok(entry)
}

/// Lists ALL authorizations of a session (pending + terminal) ordered by
/// `requested_at ASC` — the data source for rebuilding authorization bubbles
/// when a session is opened or restored.
pub fn list_by_session(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<FileAuthorizationEntry>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, role_id, op, target_path, state, requested_at, expires_at, resolved_at, resolution
         FROM file_authorizations WHERE session_id = ?1
         ORDER BY requested_at ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![session_id], row_to_entry)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Finds an authorization by id.
pub fn find_by_id(
    conn: &Connection,
    id: &str,
) -> Result<Option<FileAuthorizationEntry>, StoreError> {
    conn.query_row(
        "SELECT id, session_id, role_id, op, target_path, state, requested_at, expires_at, resolved_at, resolution
         FROM file_authorizations WHERE id = ?1",
        params![id],
        row_to_entry,
    )
    .optional()
    .map_err(StoreError::Sqlite)
}

/// Sweeper scan: strictly expired pending requests (`expires_at < now` — an
/// entry expiring exactly at `now` is not yet due).
pub fn scan_expired_pending(
    conn: &Connection,
    now: i64,
) -> Result<Vec<FileAuthorizationEntry>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, role_id, op, target_path, state, requested_at, expires_at, resolved_at, resolution
         FROM file_authorizations WHERE state = 'pending' AND expires_at < ?1
         ORDER BY expires_at ASC",
    )?;
    let rows = stmt.query_map(params![now], row_to_entry)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Transitions an authorization to `to` while appending the mirrored
/// `authorization.*` event — both inside one transaction. Returns the updated
/// entry. Rejects missing rows (`NotFound`) and illegal moves (`Conflict`).
///
/// Callers must only perform external side effects (file mutation, waking the
/// agent) AFTER this returns, i.e. after the transaction committed.
pub fn transition_with_event(
    conn: &Connection,
    id: &str,
    to: AuthorizationState,
    resolution: Option<&str>,
    event_kind: &str,
    event_payload: &serde_json::Value,
    now: i64,
) -> Result<FileAuthorizationEntry, StoreError> {
    let tx = conn.unchecked_transaction()?;
    let current = tx
        .query_row(
            "SELECT id, session_id, role_id, op, target_path, state, requested_at, expires_at, resolved_at, resolution
             FROM file_authorizations WHERE id = ?1",
            params![id],
            row_to_entry,
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound {
            entity: ENTITY,
            id: id.to_string(),
        })?;
    if !current.state.can_transition(to) {
        return Err(StoreError::Conflict {
            entity: ENTITY,
            id: id.to_string(),
            expected: AuthorizationState::Pending.as_str().to_string(),
        });
    }
    events::append(
        &tx,
        "session",
        &current.session_id,
        event_kind,
        event_payload,
        now,
    )?;
    tx.execute(
        "UPDATE file_authorizations
         SET state = ?2, resolved_at = ?3, resolution = ?4
         WHERE id = ?1",
        params![id, to.as_str(), now, resolution],
    )?;
    tx.commit()?;
    Ok(FileAuthorizationEntry {
        state: to,
        resolved_at: Some(now),
        resolution: resolution.map(str::to_string),
        ..current
    })
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileAuthorizationEntry> {
    let op: String = row.get(3)?;
    let state: String = row.get(5)?;
    Ok(FileAuthorizationEntry {
        id: row.get(0)?,
        session_id: row.get(1)?,
        role_id: row.get(2)?,
        op: AuthorizationOp::parse(&op).unwrap_or(AuthorizationOp::Write),
        target_path: row.get(4)?,
        state: AuthorizationState::parse(&state).unwrap_or(AuthorizationState::Pending),
        requested_at: row.get(6)?,
        expires_at: row.get(7)?,
        resolved_at: row.get(8)?,
        resolution: row.get(9)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Session;
    use crate::store::migrations;
    use crate::store::repos::sessions;
    use serde_json::json;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    fn seed_session(conn: &Connection, id: &str) {
        sessions::insert(conn, &Session::new_chat(id.into(), "t".into(), 1)).unwrap();
    }

    fn pending(conn: &Connection, session_id: &str, path: &str) -> FileAuthorizationEntry {
        insert_pending(
            conn,
            session_id,
            "role_1",
            AuthorizationOp::Write,
            path,
            120_000,
        )
        .unwrap()
    }

    #[test]
    fn insert_and_list_roundtrip() {
        let conn = db();
        seed_session(&conn, "s1");
        let a = pending(&conn, "s1", "C:\\proj\\a.txt");
        let b = insert_pending(
            &conn,
            "s1",
            "role_2",
            AuthorizationOp::Delete,
            "C:\\proj\\b.txt",
            60_000,
        )
        .unwrap();
        assert!(uuid::Uuid::parse_str(&a.id).is_ok());
        assert_eq!(a.state, AuthorizationState::Pending);
        assert_eq!(b.op, AuthorizationOp::Delete);
        assert!(b.expires_at > b.requested_at);
        let all = list_by_session(&conn, "s1").unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, a.id);
        assert_eq!(
            find_by_id(&conn, &a.id).unwrap().unwrap().target_path,
            "C:\\proj\\a.txt"
        );
        assert!(list_by_session(&conn, "ghost").unwrap().is_empty());
    }

    #[test]
    fn scan_boundary_expires_at_equal_not_hit() {
        let conn = db();
        seed_session(&conn, "s1");
        let a = pending(&conn, "s1", "C:\\proj\\a.txt");
        // expires_at 相等不命中（严格小于）
        assert!(scan_expired_pending(&conn, a.expires_at)
            .unwrap()
            .is_empty());
        // 过 1ms 后命中
        let due = scan_expired_pending(&conn, a.expires_at + 1).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, a.id);
    }

    #[test]
    fn transition_appends_event_and_updates_row_atomically() {
        let conn = db();
        seed_session(&conn, "s1");
        let a = pending(&conn, "s1", "C:\\proj\\a.txt");
        let updated = transition_with_event(
            &conn,
            &a.id,
            AuthorizationState::GrantedOnce,
            Some("user"),
            "authorization.resolved",
            &json!({ "authorizationId": a.id, "decision": "grant_once" }),
            a.requested_at + 5,
        )
        .unwrap();
        assert_eq!(updated.state, AuthorizationState::GrantedOnce);
        assert_eq!(updated.resolved_at, Some(a.requested_at + 5));
        let stored = find_by_id(&conn, &a.id).unwrap().unwrap();
        assert_eq!(stored.state, AuthorizationState::GrantedOnce);
        assert_eq!(stored.resolution.as_deref(), Some("user"));
        // 事件已随状态更新同事务落库
        let evs = events::list_by_aggregate(&conn, "session", "s1", None).unwrap();
        let auth_events: Vec<_> = evs
            .iter()
            .filter(|e| e.kind == "authorization.resolved")
            .collect();
        assert_eq!(auth_events.len(), 1);
        assert_eq!(auth_events[0].payload["authorizationId"], a.id);
    }

    #[test]
    fn terminal_states_reject_further_transitions() {
        let conn = db();
        seed_session(&conn, "s1");
        let a = pending(&conn, "s1", "C:\\proj\\a.txt");
        transition_with_event(
            &conn,
            &a.id,
            AuthorizationState::Denied,
            Some("user"),
            "authorization.resolved",
            &json!({}),
            10,
        )
        .unwrap();
        // denied 是终态：再迁移（含同向）一律拒绝
        for to in [
            AuthorizationState::GrantedOnce,
            AuthorizationState::GrantedSession,
            AuthorizationState::Denied,
            AuthorizationState::TimeoutDefault,
            AuthorizationState::Voided,
        ] {
            assert!(
                matches!(
                    transition_with_event(
                        &conn,
                        &a.id,
                        to,
                        None,
                        "authorization.resolved",
                        &json!({}),
                        11
                    ),
                    Err(StoreError::Conflict { .. })
                ),
                "terminal -> {to:?} must be rejected"
            );
        }
        // 状态行未变
        assert_eq!(
            find_by_id(&conn, &a.id).unwrap().unwrap().state,
            AuthorizationState::Denied
        );
    }

    #[test]
    fn missing_id_is_not_found() {
        let conn = db();
        seed_session(&conn, "s1");
        assert!(matches!(
            transition_with_event(
                &conn,
                "ghost",
                AuthorizationState::Denied,
                None,
                "authorization.resolved",
                &json!({}),
                1
            ),
            Err(StoreError::NotFound {
                entity: "file_authorization",
                ..
            })
        ));
    }

    #[test]
    fn rows_cascade_with_session_delete() {
        let conn = db();
        seed_session(&conn, "s1");
        pending(&conn, "s1", "C:\\proj\\a.txt");
        assert_eq!(list_by_session(&conn, "s1").unwrap().len(), 1);
        conn.execute("DELETE FROM sessions WHERE id = 's1'", [])
            .unwrap();
        assert!(list_by_session(&conn, "s1").unwrap().is_empty());
    }
}
