//! Session repository.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{AgentRefKind, ConversationKind, Session};
use crate::store::StoreError;

pub fn insert(conn: &Connection, session: &Session) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO sessions (id, title, created_at, updated_at, kind, agent_kind, agent_ref_id, team_id, task_id, schedule_id, goal, main_agent_id, route_mode, whiteboard_route_mode)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            session.id,
            session.title,
            session.created_at,
            session.updated_at,
            session.kind.as_str(),
            session.agent.as_ref().map(|(k, _)| k.as_str()),
            session.agent.as_ref().map(|(_, id)| id.as_str()),
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
        "SELECT id, title, created_at, updated_at, kind, agent_kind, agent_ref_id, team_id, task_id, schedule_id, goal, main_agent_id, route_mode, whiteboard_route_mode
         FROM sessions WHERE id = ?1",
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

/// Updates the agent binding on a session. `None` clears the binding.
pub fn update_agent(
    conn: &Connection,
    id: &str,
    agent: Option<(AgentRefKind, &str)>,
    at: i64,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE sessions SET agent_kind = ?2, agent_ref_id = ?3, updated_at = ?4 WHERE id = ?1",
        params![
            id,
            agent.map(|(k, _)| k.as_str()),
            agent.as_ref().map(|(_, ref_id)| ref_id),
            at,
        ],
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
pub fn update_kind(
    conn: &Connection,
    id: &str,
    kind: ConversationKind,
) -> Result<(), StoreError> {
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
pub fn list(
    conn: &Connection,
    workspace_id: &str,
    limit: u32,
) -> Result<Vec<Session>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, title, created_at, updated_at, kind, agent_kind, agent_ref_id, team_id, task_id, schedule_id, goal, main_agent_id, route_mode, whiteboard_route_mode
         FROM sessions WHERE workspace_id = ?1 ORDER BY updated_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![workspace_id, limit], row_to_session)?;
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

fn row_to_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    let kind_str: String = row.get(4)?;
    let kind = ConversationKind::parse(&kind_str).unwrap_or(ConversationKind::Chat);
    let agent_kind: Option<String> = row.get(5)?;
    let agent_ref_id: Option<String> = row.get(6)?;
    let agent = match (agent_kind.as_deref(), agent_ref_id) {
        (Some(k), Some(id)) => AgentRefKind::parse(k).map(|kind| (kind, id)),
        _ => None,
    };
    Ok(Session {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
        kind,
        agent,
        team_id: row.get(7)?,
        task_id: row.get(8)?,
        schedule_id: row.get(9)?,
        goal: row.get(10)?,
        main_agent_id: row.get(11)?,
        route_mode: row.get(12)?,
        whiteboard_route_mode: row.get(13)?,
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
}
