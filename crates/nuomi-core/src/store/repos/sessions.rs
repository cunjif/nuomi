//! Session repository.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{AgentRefKind, ConversationKind, Session};
use crate::store::StoreError;

pub fn insert(conn: &Connection, session: &Session) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO sessions (id, title, created_at, updated_at, kind, team_id, task_id, schedule_id, goal, main_agent_id, route_mode, whiteboard_route_mode)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            session.id,
            session.title,
            session.created_at,
            session.updated_at,
            session.kind.as_str(),
            session.team_id,
            session.task_id,
            session.schedule_id,
            session.goal,
            session.main_agent_id,
            session.route_mode,
            session.whiteboard_route_mode,
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Session, StoreError> {
    conn.query_row(
        "SELECT id, title, created_at, updated_at, kind, team_id, task_id, schedule_id, goal, main_agent_id, route_mode, whiteboard_route_mode, deleted_at
         FROM sessions WHERE id = ?1 AND deleted_at IS NULL",
        params![id],
        row_to_session,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "session",
        id: id.to_string(),
    })
}

pub fn touch(conn: &Connection, id: &str, at: i64) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE sessions SET updated_at = ?2 WHERE id = ?1",
        params![id, at],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "session",
            id: id.to_string(),
        });
    }
    Ok(())
}

pub fn update_title(conn: &Connection, id: &str, title: &str) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE sessions SET title = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, title, crate::domain::now_ms()],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "session",
            id: id.to_string(),
        });
    }
    Ok(())
}

/// Updates conversation metadata (goal, main_agent_id, route_mode, whiteboard_route_mode).
pub fn update_meta(
    conn: &Connection,
    id: &str,
    goal: Option<&str>,
    main_agent_id: Option<&str>,
    route_mode: Option<&str>,
    whiteboard_route_mode: Option<&str>,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE sessions SET goal = ?2, main_agent_id = ?3, route_mode = ?4, whiteboard_route_mode = ?5, updated_at = ?6 WHERE id = ?1",
        params![id, goal, main_agent_id, route_mode, whiteboard_route_mode, crate::domain::now_ms()],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "session",
            id: id.to_string(),
        });
    }
    Ok(())
}

/// Upgrades a session's kind (e.g. chat → group) and sets updated_at.
pub fn update_kind(conn: &Connection, id: &str, kind: ConversationKind) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE sessions SET kind = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, kind.as_str(), crate::domain::now_ms()],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "session",
            id: id.to_string(),
        });
    }
    Ok(())
}

// ---------------------------------------------------- participants

/// Adds an agent to a conversation's participant set. Idempotent on PK conflict.
pub fn add_participant(
    conn: &Connection,
    session_id: &str,
    agent_kind: AgentRefKind,
    agent_ref_id: &str,
    joined_at: i64,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT OR IGNORE INTO conversation_participants (session_id, agent_kind, agent_ref_id, joined_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![session_id, agent_kind.as_str(), agent_ref_id, joined_at],
    )?;
    Ok(())
}

/// Lists all participant agents for a conversation.
pub fn list_participants(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<(AgentRefKind, String)>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT agent_kind, agent_ref_id FROM conversation_participants WHERE session_id = ?1 ORDER BY joined_at ASC",
    )?;
    let rows = stmt.query_map(params![session_id], |row| {
        let kind_str: String = row.get(0)?;
        let ref_id: String = row.get(1)?;
        let kind = AgentRefKind::parse(&kind_str).unwrap_or(AgentRefKind::Cli);
        Ok((kind, ref_id))
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Checks whether an agent is already a participant.
pub fn is_participant(
    conn: &Connection,
    session_id: &str,
    agent_kind: AgentRefKind,
    agent_ref_id: &str,
) -> Result<bool, StoreError> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM conversation_participants WHERE session_id = ?1 AND agent_kind = ?2 AND agent_ref_id = ?3",
        params![session_id, agent_kind.as_str(), agent_ref_id],
        |row| row.get(0),
    )?;
    Ok(n > 0)
}

/// Removes a single participant from a conversation. No-op if not present.
pub fn remove_participant(
    conn: &Connection,
    session_id: &str,
    agent_kind: AgentRefKind,
    agent_ref_id: &str,
) -> Result<(), StoreError> {
    conn.execute(
        "DELETE FROM conversation_participants WHERE session_id = ?1 AND agent_kind = ?2 AND agent_ref_id = ?3",
        params![session_id, agent_kind.as_str(), agent_ref_id],
    )?;
    Ok(())
}

/// Clears all participants from a conversation. Used by `set_conversation_participants`
/// for whole-list replacement.
pub fn clear_participants(conn: &Connection, session_id: &str) -> Result<(), StoreError> {
    conn.execute(
        "DELETE FROM conversation_participants WHERE session_id = ?1",
        params![session_id],
    )?;
    Ok(())
}

/// Soft-deletes a session (ADR 0014). Marks `deleted_at` on the session row;
/// all dependent rows (events, participants, CLI handles, todos) are preserved
/// so the deletion is recoverable and the `events` append-only invariant holds.
/// Idempotent: a no-op if the session is already soft-deleted or does not exist.
pub fn delete(conn: &Connection, session_id: &str) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE sessions SET deleted_at = ?2 WHERE id = ?1 AND deleted_at IS NULL",
        params![session_id, crate::domain::now_ms()],
    )?;
    Ok(())
}

/// Soft-deletes all sessions for a workspace. Returns the number of sessions
/// newly marked deleted. Each session is cascaded via [`delete`].
pub fn delete_all_for_workspace(
    conn: &Connection,
    workspace_id: &str,
) -> Result<usize, StoreError> {
    let session_ids: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT id FROM sessions WHERE workspace_id = ?1 AND deleted_at IS NULL")?;
        let rows = stmt.query_map(params![workspace_id], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let count = session_ids.len();
    for sid in &session_ids {
        delete(conn, sid)?;
    }
    Ok(count)
}

/// Soft-deletes all sessions across all workspaces. Returns the number of
/// sessions newly marked deleted. Each session is cascaded via [`delete`].
pub fn delete_all(conn: &Connection) -> Result<usize, StoreError> {
    let session_ids: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM sessions WHERE deleted_at IS NULL")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let count = session_ids.len();
    for sid in &session_ids {
        delete(conn, sid)?;
    }
    Ok(count)
}

// ---------------------------------------------------- busy state (ADR 0015)

/// Marks a session's agent as busy (1) or idle (0).
pub fn set_busy(conn: &Connection, session_id: &str, busy: bool) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE sessions SET agent_busy = ?2 WHERE id = ?1",
        params![session_id, busy as i64],
    )?;
    Ok(())
}

/// Atomically transitions a session from idle to busy. Returns `true` if the
/// transition succeeded (caller owns the turn), `false` if already busy.
pub fn try_set_busy(conn: &Connection, session_id: &str) -> Result<bool, StoreError> {
    let n = conn.execute(
        "UPDATE sessions SET agent_busy = 1 WHERE id = ?1 AND agent_busy = 0",
        params![session_id],
    )?;
    Ok(n > 0)
}

/// Returns whether the session's agent is currently busy.
pub fn get_busy(conn: &Connection, session_id: &str) -> Result<bool, StoreError> {
    let busy: i64 = conn.query_row(
        "SELECT agent_busy FROM sessions WHERE id = ?1",
        params![session_id],
        |row| row.get(0),
    )?;
    Ok(busy != 0)
}

// ---------------------------------------------------- todos

/// Inserts a todo item for a conversation.
pub fn insert_todo(conn: &Connection, todo: &crate::domain::TodoItem) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO conversation_todos (id, session_id, description, completed, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            todo.id,
            todo.session_id,
            todo.description,
            todo.completed as i64,
            todo.created_at,
            todo.updated_at,
        ],
    )?;
    Ok(())
}

/// Lists todo items for a conversation.
pub fn list_todos(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<crate::domain::TodoItem>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, description, completed, created_at, updated_at FROM conversation_todos WHERE session_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![session_id], |row| {
        let completed: i64 = row.get(3)?;
        Ok(crate::domain::TodoItem {
            id: row.get(0)?,
            session_id: row.get(1)?,
            description: row.get(2)?,
            completed: completed != 0,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

// ---------------------------------------------------- cache scope

/// Sets the session's cache-lineage scope (hermes cache-lineage root).
/// An empty scope means "unset"; consumers fall back to the session id.
pub fn set_cache_scope(conn: &Connection, id: &str, scope: &str) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE sessions SET cache_scope = ?2 WHERE id = ?1",
        params![id, scope],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "session",
            id: id.to_string(),
        });
    }
    Ok(())
}

/// Reads the cache-lineage scope. `Ok(None)` means unset (empty string),
/// which semantically falls back to the session id.
pub fn cache_scope(conn: &Connection, id: &str) -> Result<Option<String>, StoreError> {
    let scope: String = conn
        .query_row(
            "SELECT cache_scope FROM sessions WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| StoreError::NotFound {
            entity: "session",
            id: id.to_string(),
        })?;
    Ok((!scope.is_empty()).then_some(scope))
}

/// Lists sessions for `workspace_id`, most recently updated first.
/// Cross-workspace isolation: only sessions belonging to this workspace are returned.
/// Steward sessions are physically excluded (their authoritative marker is a row in
/// `steward_sessions`; genuinely background sessions are unaffected).
pub fn list(conn: &Connection, workspace_id: &str, limit: u32) -> Result<Vec<Session>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, title, created_at, updated_at, kind, team_id, task_id, schedule_id, goal, main_agent_id, route_mode, whiteboard_route_mode, deleted_at
         FROM sessions WHERE workspace_id = ?1 AND deleted_at IS NULL
         AND NOT EXISTS (SELECT 1 FROM steward_sessions WHERE id = sessions.id)
         ORDER BY updated_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![workspace_id, limit], row_to_session)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Semantic alias for [`list`] — lists sessions belonging to a single workspace.
pub fn list_by_workspace(
    conn: &Connection,
    workspace_id: &str,
    limit: u32,
) -> Result<Vec<Session>, StoreError> {
    list(conn, workspace_id, limit)
}

/// Lists sessions across ALL workspaces, most recently updated first.
/// Used by the conversation list when the workspace filter is "all".
/// Steward sessions are physically excluded (see [`list`]).
pub fn list_all(conn: &Connection, limit: u32) -> Result<Vec<Session>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, title, created_at, updated_at, kind, team_id, task_id, schedule_id, goal, main_agent_id, route_mode, whiteboard_route_mode, deleted_at
         FROM sessions WHERE deleted_at IS NULL
         AND NOT EXISTS (SELECT 1 FROM steward_sessions WHERE id = sessions.id)
         ORDER BY updated_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], row_to_session)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Sets the workspace isolation id on a session (used after `insert` to bind
/// a new session to the currently active workspace).
pub fn set_workspace_id(
    conn: &Connection,
    session_id: &str,
    workspace_id: &str,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE sessions SET workspace_id = ?2 WHERE id = ?1",
        params![session_id, workspace_id],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "session",
            id: session_id.to_string(),
        });
    }
    Ok(())
}

// ---------------------------------------------------- work directory binding (0040)

/// Bind-validation failure — surfaces as IPC code `session.invalid_directory`
/// (never silently ignored, spec 6.4-2).
#[derive(Debug, thiserror::Error)]
pub enum BindingError {
    #[error("work directory {directory_id} cannot bind in workspace {workspace_id}")]
    InvalidDirectory {
        workspace_id: String,
        directory_id: String,
    },
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl BindingError {
    /// Extracts the underlying store error (for cross-service `From` chains
    /// that only know `StoreError`).
    pub fn into_store_error(self) -> StoreError {
        match self {
            BindingError::Store(err) => err,
            BindingError::InvalidDirectory {
                workspace_id,
                directory_id,
            } => StoreError::Conflict {
                entity: "session_directory_binding",
                id: format!("{workspace_id}/{directory_id}"),
                expected: "valid mounted directory in workspace".to_string(),
            },
        }
    }
}

/// A session's work-directory binding with the derived availability. The
/// binding row is append-only: a directory that was removed or unmounted
/// reads `available=false` but the binding itself stays (never re-deleted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionWorkDirectoryBinding {
    pub work_directory_id: String,
    /// Directory row details — `None` when the row itself is gone.
    pub directory: Option<crate::store::repos::work_directories::WorkDirectoryEntry>,
    /// Directory row exists AND is mounted.
    pub available: bool,
}

/// Validates that `work_directory_id` belongs to `workspace_id` and is
/// mounted. Callers embedding creation in one transaction use this plus their
/// own INSERT; `set_work_directory` uses it internally too.
pub fn validate_work_directory(
    conn: &Connection,
    workspace_id: &str,
    work_directory_id: &str,
) -> Result<(), BindingError> {
    let invalid = || BindingError::InvalidDirectory {
        workspace_id: workspace_id.to_string(),
        directory_id: work_directory_id.to_string(),
    };
    let owner: Option<String> = conn
        .query_row(
            "SELECT workspace_id FROM work_directories WHERE id = ?1",
            params![work_directory_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(StoreError::Sqlite)?;
    let Some(owner_workspace) = owner else {
        return Err(invalid());
    };
    if owner_workspace != workspace_id {
        return Err(invalid());
    }
    let mounted: i64 = conn
        .query_row(
            "SELECT mounted FROM work_directories WHERE id = ?1",
            params![work_directory_id],
            |r| r.get(0),
        )
        .map_err(StoreError::Sqlite)?;
    if mounted == 0 {
        return Err(invalid());
    }
    Ok(())
}

/// Binds a session to a work directory. The session must exist and belong to
/// the same workspace as the directory, which must be mounted — otherwise
/// [`BindingError::InvalidDirectory`] (repo-layer transactional check,
/// migration 0040 conventions).
pub fn set_work_directory(
    conn: &Connection,
    session_id: &str,
    work_directory_id: &str,
) -> Result<(), BindingError> {
    let tx = conn.unchecked_transaction().map_err(StoreError::Sqlite)?;
    let workspace_id: String = tx
        .query_row(
            "SELECT workspace_id FROM sessions WHERE id = ?1",
            params![session_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(StoreError::Sqlite)?
        .ok_or_else(|| StoreError::NotFound {
            entity: "session",
            id: session_id.to_string(),
        })?;
    validate_work_directory(&tx, &workspace_id, work_directory_id)?;
    let n = tx
        .execute(
            "UPDATE sessions SET work_directory_id = ?2, updated_at = ?3 WHERE id = ?1",
            params![session_id, work_directory_id, crate::domain::now_ms()],
        )
        .map_err(StoreError::Sqlite)?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "session",
            id: session_id.to_string(),
        }
        .into());
    }
    tx.commit().map_err(StoreError::Sqlite)?;
    Ok(())
}

/// Clears a session's work-directory binding (unbind). No-op on missing rows.
pub fn clear_work_directory(conn: &Connection, session_id: &str) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE sessions SET work_directory_id = NULL WHERE id = ?1",
        params![session_id],
    )?;
    Ok(())
}

/// Reads a session's binding with the derived availability (NULL column →
/// `None`; directory row gone → `available=false`, binding kept).
pub fn get_work_directory_binding(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<SessionWorkDirectoryBinding>, StoreError> {
    let bound: Option<String> = conn
        .query_row(
            "SELECT work_directory_id FROM sessions WHERE id = ?1",
            params![session_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let Some(work_directory_id) = bound else {
        return Ok(None);
    };
    let directory = crate::store::repos::work_directories::find_by_id(conn, &work_directory_id)?;
    let available = directory.as_ref().map(|d| d.mounted).unwrap_or(false);
    Ok(Some(SessionWorkDirectoryBinding {
        work_directory_id,
        directory,
        available,
    }))
}

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    let kind_str: String = row.get(4)?;
    let kind = ConversationKind::parse(&kind_str).unwrap_or(ConversationKind::Chat);
    Ok(Session {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
        kind,
        team_id: row.get(5)?,
        task_id: row.get(6)?,
        schedule_id: row.get(7)?,
        goal: row.get(8)?,
        main_agent_id: row.get(9)?,
        route_mode: row.get(10)?,
        whiteboard_route_mode: row.get(11)?,
        deleted_at: row.get(12)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        ensure_cache_scope_column(&conn);
        conn
    }

    /// Migration 0007 is pending registration in `store/migrations.rs`
    /// (main session owns it), so make sure the column exists for tests.
    /// Once registered this becomes a no-op.
    fn ensure_cache_scope_column(conn: &Connection) {
        let has_column: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('sessions') WHERE name = 'cache_scope'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        if has_column == 0 {
            conn.execute(
                "ALTER TABLE sessions ADD COLUMN cache_scope TEXT NOT NULL DEFAULT ''",
                [],
            )
            .unwrap();
        }
    }

    fn session(id: &str) -> Session {
        Session::new_chat(id.into(), "t".into(), 1)
    }

    #[test]
    fn insert_get_roundtrip() {
        let conn = db();
        insert(&conn, &session("s1")).unwrap();
        assert_eq!(get(&conn, "s1").unwrap().id, "s1");
    }

    #[test]
    fn get_missing_is_not_found() {
        let conn = db();
        assert!(matches!(
            get(&conn, "nope"),
            Err(StoreError::NotFound {
                entity: "session",
                ..
            })
        ));
    }

    #[test]
    fn touch_updates_and_lists_recency_order() {
        let conn = db();
        insert(&conn, &session("a")).unwrap();
        insert(&conn, &session("b")).unwrap();
        touch(&conn, "a", 99).unwrap();
        let list = list(&conn, "__migrated__", 10).unwrap();
        assert_eq!(list[0].id, "a");
        assert_eq!(list[0].updated_at, 99);
    }

    #[test]
    fn duplicate_insert_fails() {
        let conn = db();
        insert(&conn, &session("dup")).unwrap();
        assert!(insert(&conn, &session("dup")).is_err());
    }

    #[test]
    fn update_title_sets_title_and_missing_id_is_not_found() {
        let conn = db();
        insert(&conn, &session("s1")).unwrap();
        update_title(&conn, "s1", "renamed").unwrap();
        assert_eq!(get(&conn, "s1").unwrap().title, "renamed");
        assert!(matches!(
            update_title(&conn, "nope", "x"),
            Err(StoreError::NotFound {
                entity: "session",
                ..
            })
        ));
    }

    #[test]
    fn cache_scope_roundtrip_and_empty_means_unset() {
        let conn = db();
        insert(&conn, &session("s1")).unwrap();
        assert_eq!(cache_scope(&conn, "s1").unwrap(), None);
        set_cache_scope(&conn, "s1", "lineage-root").unwrap();
        assert_eq!(
            cache_scope(&conn, "s1").unwrap(),
            Some("lineage-root".into())
        );
        set_cache_scope(&conn, "s1", "").unwrap();
        assert_eq!(cache_scope(&conn, "s1").unwrap(), None);
    }

    #[test]
    fn cache_scope_missing_session_is_not_found() {
        let conn = db();
        assert!(matches!(
            cache_scope(&conn, "nope"),
            Err(StoreError::NotFound {
                entity: "session",
                ..
            })
        ));
        assert!(matches!(
            set_cache_scope(&conn, "nope", "s"),
            Err(StoreError::NotFound {
                entity: "session",
                ..
            })
        ));
    }

    #[test]
    fn meta_fields_roundtrip() {
        let conn = db();
        insert(&conn, &session("s1")).unwrap();
        update_meta(
            &conn,
            "s1",
            Some("Build a calculator"),
            Some("cli:agent-1"),
            Some("orchestrator_worker"),
            Some("preemptive"),
        )
        .unwrap();
        let s = get(&conn, "s1").unwrap();
        assert_eq!(s.goal.as_deref(), Some("Build a calculator"));
        assert_eq!(s.main_agent_id.as_deref(), Some("cli:agent-1"));
        assert_eq!(s.route_mode.as_deref(), Some("orchestrator_worker"));
        assert_eq!(s.whiteboard_route_mode.as_deref(), Some("preemptive"));
    }

    #[test]
    fn participants_add_list_check() {
        let conn = db();
        insert(&conn, &session("s1")).unwrap();
        add_participant(&conn, "s1", AgentRefKind::Cli, "agent-1", 100).unwrap();
        add_participant(&conn, "s1", AgentRefKind::Role, "role-1", 200).unwrap();
        // Idempotent.
        add_participant(&conn, "s1", AgentRefKind::Cli, "agent-1", 300).unwrap();
        let parts = list_participants(&conn, "s1").unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], (AgentRefKind::Cli, "agent-1".into()));
        assert_eq!(parts[1], (AgentRefKind::Role, "role-1".into()));
        assert!(is_participant(&conn, "s1", AgentRefKind::Cli, "agent-1").unwrap());
        assert!(!is_participant(&conn, "s1", AgentRefKind::Cli, "agent-2").unwrap());
    }

    #[test]
    fn todos_insert_list() {
        let conn = db();
        insert(&conn, &session("s1")).unwrap();
        let now = crate::domain::now_ms();
        insert_todo(
            &conn,
            &crate::domain::TodoItem {
                id: "todo-1".into(),
                session_id: "s1".into(),
                description: "Parse input".into(),
                completed: false,
                created_at: now,
                updated_at: now,
            },
        )
        .unwrap();
        insert_todo(
            &conn,
            &crate::domain::TodoItem {
                id: "todo-2".into(),
                session_id: "s1".into(),
                description: "Return result".into(),
                completed: true,
                created_at: now + 1,
                updated_at: now + 1,
            },
        )
        .unwrap();
        let todos = list_todos(&conn, "s1").unwrap();
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].description, "Parse input");
        assert!(!todos[0].completed);
        assert_eq!(todos[1].description, "Return result");
        assert!(todos[1].completed);
    }

    #[test]
    fn update_kind_upgrades_chat_to_group() {
        let conn = db();
        insert(&conn, &session("s1")).unwrap();
        update_kind(&conn, "s1", ConversationKind::Group).unwrap();
        let s = get(&conn, "s1").unwrap();
        assert_eq!(s.kind, ConversationKind::Group);
    }

    #[test]
    fn list_isolates_by_workspace_id() {
        let conn = db();
        // Two sessions: one in workspace A, one in workspace B.
        insert(&conn, &session("sa")).unwrap();
        set_workspace_id(&conn, "sa", "ws-a").unwrap();
        insert(&conn, &session("sb")).unwrap();
        set_workspace_id(&conn, "sb", "ws-b").unwrap();
        // Workspace A sees only sa.
        let a_list = list(&conn, "ws-a", 10).unwrap();
        assert_eq!(a_list.len(), 1);
        assert_eq!(a_list[0].id, "sa");
        // Workspace B sees only sb.
        let b_list = list(&conn, "ws-b", 10).unwrap();
        assert_eq!(b_list.len(), 1);
        assert_eq!(b_list[0].id, "sb");
        // Nonexistent workspace sees nothing.
        let empty = list(&conn, "ws-none", 10).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn set_workspace_id_missing_session_is_not_found() {
        let conn = db();
        assert!(matches!(
            set_workspace_id(&conn, "nope", "ws-x"),
            Err(StoreError::NotFound {
                entity: "session",
                ..
            })
        ));
    }

    #[test]
    fn list_excludes_steward_sessions_but_keeps_background_kind() {
        let conn = db();
        // Three sessions in the same workspace: a regular IM chat, a genuine
        // background session (kind='background', no steward_sessions row), and a
        // steward session (sessions row + authoritative steward_sessions marker).
        insert(&conn, &session("im-chat")).unwrap();
        set_workspace_id(&conn, "im-chat", "ws-a").unwrap();
        insert(&conn, &session("bg-real")).unwrap();
        set_workspace_id(&conn, "bg-real", "ws-a").unwrap();
        update_kind(&conn, "bg-real", ConversationKind::Background).unwrap();
        insert(&conn, &session("steward-sess")).unwrap();
        set_workspace_id(&conn, "steward-sess", "ws-a").unwrap();
        update_kind(&conn, "steward-sess", ConversationKind::Background).unwrap();
        conn.execute(
            "INSERT INTO steward_sessions (id, steward_id, title, created_at, updated_at)
             VALUES ('steward-sess', 'steward-main', '', 1, 1)",
            [],
        )
        .unwrap();

        let ids = |sessions: &[Session]| -> Vec<String> {
            sessions.iter().map(|s| s.id.clone()).collect::<Vec<_>>()
        };

        // list excludes the steward session, keeps the genuine background one.
        let listed = list(&conn, "ws-a", 10).unwrap();
        assert!(!ids(&listed).contains(&"steward-sess".to_string()));
        assert!(ids(&listed).contains(&"im-chat".to_string()));
        assert!(ids(&listed).contains(&"bg-real".to_string()));
        assert_eq!(listed.len(), 2);

        // list_by_workspace shares the same exclusion (semantic alias).
        let by_ws = list_by_workspace(&conn, "ws-a", 10).unwrap();
        assert!(!ids(&by_ws).contains(&"steward-sess".to_string()));
        assert_eq!(by_ws.len(), 2);

        // list_all (cross-workspace "all" filter) also excludes the steward session.
        insert(&conn, &session("other-ws")).unwrap();
        set_workspace_id(&conn, "other-ws", "ws-b").unwrap();
        let all = list_all(&conn, 10).unwrap();
        assert!(!ids(&all).contains(&"steward-sess".to_string()));
        assert!(ids(&all).contains(&"other-ws".to_string()));
        assert_eq!(all.len(), 3);
    }

    // ---------------------------------------------------- work directory binding (0040)

    use crate::store::repos::work_directories as work_dirs;
    use crate::store::repos::workspaces::{self, WorkspaceEntry};

    fn seed_workspace_with_dir(conn: &Connection, ws: &str, dir: &str, path: &str) {
        workspaces::insert(
            conn,
            &WorkspaceEntry {
                id: ws.into(),
                root_path: path.into(),
                color_tag: "paper-yellow".into(),
                created_at: 1,
                is_active: false,
                is_pinned: false,
            },
        )
        .unwrap();
        work_dirs::add(conn, ws, path, dir).unwrap();
    }

    #[test]
    fn work_directory_binding_roundtrip() {
        let conn = db();
        seed_workspace_with_dir(&conn, "w1", "alpha", "C:\\proj\\alpha");
        insert(&conn, &session("s1")).unwrap();
        set_workspace_id(&conn, "s1", "w1").unwrap();
        // 存量会话（未绑定）→ None
        assert!(get_work_directory_binding(&conn, "s1").unwrap().is_none());
        set_work_directory(
            &conn,
            "s1",
            &work_dirs::find_by_path(&conn, "C:\\proj\\alpha")
                .unwrap()
                .unwrap()
                .id,
        )
        .unwrap();
        let binding = get_work_directory_binding(&conn, "s1").unwrap().unwrap();
        assert!(binding.available);
        assert_eq!(binding.directory.as_ref().unwrap().workspace_id, "w1");
        assert_eq!(
            binding.work_directory_id,
            work_dirs::find_by_path(&conn, "C:\\proj\\alpha")
                .unwrap()
                .unwrap()
                .id
        );
        // 解绑 → None
        clear_work_directory(&conn, "s1").unwrap();
        assert!(get_work_directory_binding(&conn, "s1").unwrap().is_none());
    }

    #[test]
    fn binding_rejects_directory_from_other_workspace() {
        let conn = db();
        seed_workspace_with_dir(&conn, "w1", "alpha", "C:\\proj\\alpha");
        seed_workspace_with_dir(&conn, "w2", "beta", "C:\\proj\\beta");
        insert(&conn, &session("s1")).unwrap();
        set_workspace_id(&conn, "s1", "w1").unwrap();
        let beta_dir = work_dirs::find_by_path(&conn, "C:\\proj\\beta")
            .unwrap()
            .unwrap();
        assert!(matches!(
            set_work_directory(&conn, "s1", &beta_dir.id),
            Err(BindingError::InvalidDirectory { .. })
        ));
        assert!(get_work_directory_binding(&conn, "s1").unwrap().is_none());
    }

    #[test]
    fn binding_rejects_unmounted_directory() {
        let conn = db();
        seed_workspace_with_dir(&conn, "w1", "alpha", "C:\\proj\\alpha");
        insert(&conn, &session("s1")).unwrap();
        set_workspace_id(&conn, "s1", "w1").unwrap();
        let dir = work_dirs::find_by_path(&conn, "C:\\proj\\alpha")
            .unwrap()
            .unwrap();
        work_dirs::set_mounted(&conn, &dir.id, false).unwrap();
        assert!(matches!(
            set_work_directory(&conn, "s1", &dir.id),
            Err(BindingError::InvalidDirectory { .. })
        ));
    }

    #[test]
    fn binding_rejects_missing_directory_and_missing_session() {
        let conn = db();
        seed_workspace_with_dir(&conn, "w1", "alpha", "C:\\proj\\alpha");
        insert(&conn, &session("s1")).unwrap();
        set_workspace_id(&conn, "s1", "w1").unwrap();
        // 目录 id 不存在 → InvalidDirectory
        assert!(matches!(
            set_work_directory(&conn, "s1", "ghost-dir"),
            Err(BindingError::InvalidDirectory { .. })
        ));
        // 会话不存在 → NotFound（Store 变体）
        let dir = work_dirs::find_by_path(&conn, "C:\\proj\\alpha")
            .unwrap()
            .unwrap();
        assert!(matches!(
            set_work_directory(&conn, "ghost", &dir.id),
            Err(BindingError::Store(StoreError::NotFound {
                entity: "session",
                ..
            }))
        ));
    }

    #[test]
    fn removed_directory_marks_binding_unavailable_but_keeps_row() {
        let conn = db();
        seed_workspace_with_dir(&conn, "w1", "alpha", "C:\\proj\\alpha");
        insert(&conn, &session("s1")).unwrap();
        set_workspace_id(&conn, "s1", "w1").unwrap();
        let dir = work_dirs::find_by_path(&conn, "C:\\proj\\alpha")
            .unwrap()
            .unwrap();
        set_work_directory(&conn, "s1", &dir.id).unwrap();
        // 目录被移除（行删除）→ 绑定行保留、available=false
        work_dirs::remove(&conn, &dir.id).unwrap();
        let binding = get_work_directory_binding(&conn, "s1").unwrap().unwrap();
        assert!(!binding.available);
        assert!(binding.directory.is_none());
        assert_eq!(binding.work_directory_id, dir.id);
        // 目录卸载（行保留 mounted=0）→ available=false 但 directory 有值
        work_dirs::add(&conn, "w1", "C:\\proj\\beta", "beta").unwrap();
        let beta = work_dirs::find_by_path(&conn, "C:\\proj\\beta")
            .unwrap()
            .unwrap();
        set_work_directory(&conn, "s1", &beta.id).unwrap();
        work_dirs::set_mounted(&conn, &beta.id, false).unwrap();
        let binding = get_work_directory_binding(&conn, "s1").unwrap().unwrap();
        assert!(!binding.available);
        assert!(binding.directory.is_some());
    }
}
