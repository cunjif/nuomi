//! Conversation service: creates, binds, and resolves typed conversations.
//!
//! ADR 0013: 祛除主 Agent 概念，统一为 IM 式单聊/群聊模型。
//! 参与者统一存储在 `conversation_participants` 表，单聊 = 1 参与者，
//! 群聊 = 多参与者。`sessions.agent_*` 列已废弃（migration 0021）。

use rusqlite::Connection;

use crate::domain::{now_ms, AgentRefKind, ConversationKind, Role, Session};
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

/// Creates a new conversation (session + kind + participants).
///
/// `participants` is a slice of `(AgentRefKind, id)` pairs. Each is written
/// to `conversation_participants`. Pass an empty slice for the default
/// resolution chain (materialized on first resolve).
pub fn create_conversation(
    conn: &Connection,
    kind: ConversationKind,
    title: &str,
    participants: &[(AgentRefKind, &str)],
    team_id: Option<&str>,
    schedule_id: Option<&str>,
    workspace_id: &str,
) -> Result<Session, StoreError> {
    let now = now_ms();
    let session = Session {
        id: crate::domain::new_id(),
        title: title.to_string(),
        created_at: now,
        updated_at: now,
        kind,
        team_id: team_id.map(|s| s.to_string()),
        task_id: None,
        schedule_id: schedule_id.map(|s| s.to_string()),
        goal: None,
        main_agent_id: None,
        route_mode: None,
        whiteboard_route_mode: None,
        deleted_at: None,
    };
    sessions::insert(conn, &session)?;
    sessions::set_workspace_id(conn, &session.id, workspace_id)?;
    for (kind, id) in participants {
        sessions::add_participant(conn, &session.id, *kind, id, now)?;
    }
    Ok(session)
}

/// Resolves all participants for a session from `conversation_participants`.
///
/// If the table has no participants for this session, falls through to
/// `resolve_default_agent` and materializes the result into the table
/// (so subsequent reads are stable and don't drift with global defaults).
pub fn resolve_participants(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<ResolvedAgent>, StoreError> {
    let raw = sessions::list_participants(conn, session_id)?;
    if !raw.is_empty() {
        let mut resolved = Vec::with_capacity(raw.len());
        for (kind, id) in raw {
            if let Some(r) = resolve_ref(conn, kind, &id)? {
                resolved.push(r);
            }
        }
        return Ok(resolved);
    }
    // Fallback: materialize default agent into participants table.
    if let Some(default) = resolve_default_agent(conn)? {
        sessions::add_participant(conn, session_id, default.kind, &default.id, now_ms())?;
        Ok(vec![default])
    } else {
        Ok(Vec::new())
    }
}

/// Resolves a single agent for a session — the first participant (single-chat
/// semantics). Returns `None` if no participants. Kept for compatibility with
/// `run_conversation_turn` which dispatches on a single agent.
pub fn resolve_agent(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<ResolvedAgent>, StoreError> {
    Ok(resolve_participants(conn, session_id)?.into_iter().next())
}

/// Steps 2–4 of the resolution chain — everything that does *not* depend on
/// the session. Used by `resolve_participants` fallback and list views.
pub fn resolve_default_agent(conn: &Connection) -> Result<Option<ResolvedAgent>, StoreError> {
    if let Some(default) = settings::get(conn, DEFAULT_AGENT_KEY)? {
        if let Some((kind, id)) = parse_agent_ref(&default) {
            if let Some(resolved) = resolve_ref(conn, kind, &id)? {
                return Ok(Some(resolved));
            }
        }
    }
    // ADR 0012 D3: CLI Agent 须通过 Role 绑定才能使用，不再直接作为对话
    // 对象。默认链只选第一个 builtin 且 isRoleReady 的 Role。
    let roles_list = roles::list(conn)?;
    if let Some(role) = roles_list
        .into_iter()
        .find(|r| r.builtin && is_role_ready(r))
    {
        return Ok(Some(ResolvedAgent {
            kind: AgentRefKind::Role,
            id: role.id,
            name: role.name,
        }));
    }
    Ok(None)
}

/// A role is "ready" (usable as a Role Agent) when it binds a provider or a
/// CLI agent profile. Mirrors the frontend `isRoleReady` helper.
pub fn is_role_ready(role: &Role) -> bool {
    role.provider_id.is_some()
        || role
            .params
            .get("agent_profile_id")
            .and_then(serde_json::Value::as_str)
            .is_some()
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
    s.strip_prefix("cli:")
        .map(|id| (AgentRefKind::Cli, id.to_string()))
        .or_else(|| {
            s.strip_prefix("role:")
                .map(|id| (AgentRefKind::Role, id.to_string()))
        })
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
        let session = create_conversation(
            &conn,
            ConversationKind::Chat,
            "hello",
            &[],
            None,
            None,
            "__migrated__",
        )
        .unwrap();
        assert_eq!(session.kind, ConversationKind::Chat);
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
            &[],
            Some("team-1"),
            None,
            "__migrated__",
        )
        .unwrap();
        assert_eq!(session.kind, ConversationKind::Group);
        assert_eq!(session.team_id.as_deref(), Some("team-1"));
    }

    #[test]
    fn create_conversation_with_participants() {
        let conn = db();
        let session = create_conversation(
            &conn,
            ConversationKind::Chat,
            "s",
            &[(AgentRefKind::Role, "role-1")],
            None,
            None,
            "__migrated__",
        )
        .unwrap();
        let parts = sessions::list_participants(&conn, &session.id).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], (AgentRefKind::Role, "role-1".to_string()));
    }

    #[test]
    fn resolve_participants_falls_through_to_none_when_empty() {
        let conn = db();
        let session = create_conversation(
            &conn,
            ConversationKind::Chat,
            "s",
            &[],
            None,
            None,
            "__migrated__",
        )
        .unwrap();
        let resolved = resolve_participants(&conn, &session.id).unwrap();
        assert!(resolved.is_empty());
    }

    #[test]
    fn resolve_participants_returns_participants() {
        let conn = db();
        let session = create_conversation(
            &conn,
            ConversationKind::Chat,
            "s",
            &[(AgentRefKind::Role, "role-1")],
            None,
            None,
            "__migrated__",
        )
        .unwrap();
        // role-1 doesn't exist in db → resolve_ref returns None → filtered out.
        let resolved = resolve_participants(&conn, &session.id).unwrap();
        assert!(resolved.is_empty(), "nonexistent role ref is filtered out");
        // Raw participants are still stored.
        let raw = sessions::list_participants(&conn, &session.id).unwrap();
        assert_eq!(raw.len(), 1);
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
        let result =
            compose_user_message("hello", &[("file.txt", ".nuomi/attachments/s1/file.txt")]);
        assert!(result.contains("hello"));
        assert!(result.contains("file.txt"));
        assert!(result.contains(".nuomi/attachments/s1/file.txt"));
    }
}
