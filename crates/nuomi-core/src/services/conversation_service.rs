//! Conversation service: creates, binds, and resolves typed conversations.
//!
//! A Conversation = Session + kind + bindings (migration 0011).
//! This service is the single place that knows how to:
//!  - create a session with the right kind and bindings
//!  - resolve the effective agent via the priority chain (plan §7.1)
//!  - compose a user message with attachment references

use rusqlite::Connection;

use crate::domain::{
    now_ms, AgentRefKind, ConversationKind, Session,
};
use crate::store::repos::{agent_profiles, roles, sessions, settings};
use crate::store::StoreError;

/// Setting key for the global default agent (`"cli:<id>"` or `"role:<id>"`).
pub const DEFAULT_AGENT_KEY: &str = "conversation.default_agent";

/// An agent reference resolved with enough info for display and execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAgent {
    pub kind: AgentRefKind,
    pub id: String,
    pub name: String,
}

/// Creates a new conversation (session with kind + bindings).
///
/// `agent` is `(AgentRefKind, id)` where id references `agent_profiles.id`
/// or `roles.id`. Pass `None` for the default resolution chain.
pub fn create_conversation(
    conn: &Connection,
    kind: ConversationKind,
    title: &str,
    agent: Option<(AgentRefKind, &str)>,
    team_id: Option<&str>,
    schedule_id: Option<&str>,
) -> Result<Session, StoreError> {
    let now = now_ms();
    let session = Session {
        id: crate::domain::new_id(),
        title: title.to_string(),
        created_at: now,
        updated_at: now,
        kind,
        agent: agent.map(|(k, id)| (k, id.to_string())),
        team_id: team_id.map(|s| s.to_string()),
        task_id: None,
        schedule_id: schedule_id.map(|s| s.to_string()),
        goal: None,
        main_agent_id: None,
        route_mode: None,
        whiteboard_route_mode: None,
    };
    sessions::insert(conn, &session)?;
    Ok(session)
}

/// Sets or clears the agent binding on a session, returning the refreshed row.
pub fn set_agent(
    conn: &Connection,
    session_id: &str,
    agent: Option<(AgentRefKind, &str)>,
) -> Result<Session, StoreError> {
    sessions::update_agent(conn, session_id, agent, now_ms())?;
    sessions::get(conn, session_id)
}

/// Resolves the effective agent for a session via the priority chain:
///
/// 1. Session explicit binding (`sessions.agent_kind/agent_ref_id`)
/// 2. Global default (`app_settings: conversation.default_agent`)
/// 3. First enabled `AgentProfile`
/// 4. First builtin `Role`
/// 5. `None` (kernel default provider — current behavior)
pub fn resolve_agent(
    conn: &Connection,
    session: &Session,
) -> Result<Option<ResolvedAgent>, StoreError> {
    if let Some((kind, id)) = &session.agent {
        if let Some(resolved) = resolve_ref(conn, *kind, id)? {
            return Ok(Some(resolved));
        }
    }
    resolve_default_agent(conn)
}

/// Steps 2–4 of the [`resolve_agent`] chain — everything that does *not*
/// depend on the session. List views resolve it once and reuse it for every
/// row instead of re-running the chain per row.
pub fn resolve_default_agent(conn: &Connection) -> Result<Option<ResolvedAgent>, StoreError> {
    if let Some(default) = settings::get(conn, DEFAULT_AGENT_KEY)? {
        if let Some((kind, id)) = parse_agent_ref(&default) {
            if let Some(resolved) = resolve_ref(conn, kind, &id)? {
                return Ok(Some(resolved));
            }
        }
    }
    let profiles = agent_profiles::list(conn)?;
    if let Some(profile) = profiles.into_iter().find(|p| p.enabled) {
        return Ok(Some(ResolvedAgent {
            kind: AgentRefKind::Cli,
            id: profile.id,
            name: profile.name,
        }));
    }
    let roles_list = roles::list(conn)?;
    if let Some(role) = roles_list.into_iter().find(|r| r.builtin) {
        return Ok(Some(ResolvedAgent {
            kind: AgentRefKind::Role,
            id: role.id,
            name: role.name,
        }));
    }
    Ok(None)
}

/// Fills in the display name of an agent binding. List endpoints use this so
/// a row shows "Codex" instead of the raw `cli:<id>` reference.
pub fn name_agent_ref(
    conn: &Connection,
    agent: Option<&AgentRef>,
) -> Result<Option<ResolvedAgent>, StoreError> {
    match agent {
        Some((kind, id)) => resolve_ref(conn, *kind, id),
        None => Ok(None),
    }
}

/// `(AgentRefKind, id)` as stored on a session/schedule row.
pub type AgentRef = (AgentRefKind, String);

/// Composes a user message string with inline attachment references.
///
/// `attachments` is a slice of `(name, rel_path)` pairs. Text attachments
/// are inlined; binary/image attachments get a path reference.
pub fn compose_user_message(text: &str, attachments: &[(&str, &str)]) -> String {
    if attachments.is_empty() {
        return text.to_string();
    }
    let mut parts = vec![text.to_string()];
    for (name, path) in attachments {
        parts.push(format!("[附件: {name}]({path})"));
    }
    parts.join("\n\n")
}

// ---- helpers ----

fn resolve_ref(
    conn: &Connection,
    kind: AgentRefKind,
    id: &str,
) -> Result<Option<ResolvedAgent>, StoreError> {
    match kind {
        AgentRefKind::Cli => match agent_profiles::get(conn, id) {
            Ok(profile) if profile.enabled => Ok(Some(ResolvedAgent {
                kind: AgentRefKind::Cli,
                id: profile.id,
                name: profile.name,
            })),
            Ok(_) => Ok(None),
            Err(StoreError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e),
        },
        AgentRefKind::Role => match roles::get(conn, id) {
            Ok(role) => Ok(Some(ResolvedAgent {
                kind: AgentRefKind::Role,
                id: role.id,
                name: role.name,
            })),
            Err(StoreError::NotFound { .. }) => Ok(None),
            Err(e) => Err(e),
        },
    }
}

/// Parses `"cli:<id>"` or `"role:<id>"` into `(AgentRefKind, id)`.
fn parse_agent_ref(s: &str) -> Option<(AgentRefKind, String)> {
    if let Some(id) = s.strip_prefix("cli:") {
        Some((AgentRefKind::Cli, id.to_string()))
    } else if let Some(id) = s.strip_prefix("role:") {
        Some((AgentRefKind::Role, id.to_string()))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    #[test]
    fn create_chat_conversation_defaults() {
        let conn = db();
        let session =
            create_conversation(&conn, ConversationKind::Chat, "hello", None, None, None).unwrap();
        assert_eq!(session.kind, ConversationKind::Chat);
        assert!(session.agent.is_none());
        assert!(session.team_id.is_none());
        let loaded = sessions::get(&conn, &session.id).unwrap();
        assert_eq!(loaded.kind, ConversationKind::Chat);
    }

    #[test]
    fn create_group_conversation_with_team() {
        let conn = db();
        let session = create_conversation(
            &conn,
            ConversationKind::Group,
            "group chat",
            None,
            Some("team-1"),
            None,
        )
        .unwrap();
        assert_eq!(session.kind, ConversationKind::Group);
        assert_eq!(session.team_id.as_deref(), Some("team-1"));
    }

    #[test]
    fn set_agent_updates_binding() {
        let conn = db();
        let session =
            create_conversation(&conn, ConversationKind::Chat, "s", None, None, None).unwrap();
        let updated =
            set_agent(&conn, &session.id, Some((AgentRefKind::Cli, "agent-1"))).unwrap();
        assert_eq!(
            updated.agent,
            Some((AgentRefKind::Cli, "agent-1".to_string()))
        );
        let cleared = set_agent(&conn, &session.id, None).unwrap();
        assert!(cleared.agent.is_none());
    }

    #[test]
    fn resolve_agent_falls_through_to_none_when_empty() {
        let conn = db();
        let session =
            create_conversation(&conn, ConversationKind::Chat, "s", None, None, None).unwrap();
        let resolved = resolve_agent(&conn, &session).unwrap();
        assert!(resolved.is_none());
    }

    #[test]
    fn resolve_agent_uses_session_binding() {
        let conn = db();
        let session = create_conversation(
            &conn,
            ConversationKind::Chat,
            "s",
            Some((AgentRefKind::Role, "role-1")),
            None,
            None,
        )
        .unwrap();
        let resolved = resolve_agent(&conn, &session).unwrap();
        assert!(resolved.is_none(), "nonexistent role ref resolves to None");
    }

    #[test]
    fn parse_agent_ref_roundtrip() {
        assert_eq!(
            parse_agent_ref("cli:abc"),
            Some((AgentRefKind::Cli, "abc".to_string()))
        );
        assert_eq!(
            parse_agent_ref("role:xyz"),
            Some((AgentRefKind::Role, "xyz".to_string()))
        );
        assert_eq!(parse_agent_ref("garbage"), None);
    }

    #[test]
    fn compose_user_message_no_attachments() {
        assert_eq!(compose_user_message("hello", &[]), "hello");
    }

    #[test]
    fn compose_user_message_with_attachments() {
        let result = compose_user_message("hello", &[("file.txt", ".nuomi/attachments/s1/file.txt")]);
        assert!(result.contains("hello"));
        assert!(result.contains("file.txt"));
        assert!(result.contains(".nuomi/attachments/s1/file.txt"));
    }
}
