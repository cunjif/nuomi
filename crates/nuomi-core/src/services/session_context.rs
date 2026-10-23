//! Session-generic composer services (composer alignment): prompt
//! optimization and context-usage aggregation that work for EVERY
//! conversation surface — ordinary RoleAgent chats, group chats, and the
//! steward chat.
//!
//! Dispatch rule: a session present in `steward_sessions` keeps the
//! established steward semantics via delegation (spec 4.1-6 / 4.5);
//! ordinary sessions resolve their bound role/provider for optimization
//! and aggregate the append-only session events for usage — the same
//! `list_by_aggregate` source `rebuild_history` replays from, so the
//! estimate tracks what the next turn would actually send.
//!
//! Token estimation mirrors `prune_view` (`providers/context.rs`):
//! byte length divided by 4 (CHARS_PER_TOKEN). Ordinary sessions have no
//! prune events, so `prune_records` is always empty for them; the budget
//! is the shared reference budget (`CONTEXT_BUDGET_TOKENS`).

use std::path::PathBuf;
use std::sync::Arc;

use tokio::task::spawn_blocking;

use crate::providers::SecretStore;
use crate::services::resolve_participants;
use crate::steward::context_usage as steward_usage;
use crate::steward::llm_loop::CONTEXT_BUDGET_TOKENS;
use crate::steward::prompt_optimizer::{self, OptimizeError};
use crate::store::repos::{events as events_repo, roles, sessions, steward as steward_repo};
use crate::store::Db;

/// Cap on event rows scanned per usage aggregation (matches the steward
/// indicator's scan limit).
const EVENT_SCAN_LIMIT: usize = 500;

#[derive(Debug, thiserror::Error)]
pub enum SessionContextError {
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("store error: {0}")]
    Store(String),
}

impl From<crate::store::StoreError> for SessionContextError {
    fn from(e: crate::store::StoreError) -> Self {
        match e {
            crate::store::StoreError::NotFound { entity, id } => {
                Self::NotFound(format!("{entity}#{id}"))
            }
            other => Self::Store(other.to_string()),
        }
    }
}

impl From<crate::steward::StewardError> for SessionContextError {
    fn from(e: crate::steward::StewardError) -> Self {
        match e {
            crate::steward::StewardError::NotFound { entity, id } => {
                Self::NotFound(format!("{entity}#{id}"))
            }
            other => Self::Store(other.to_string()),
        }
    }
}

#[derive(Debug)]
pub struct SessionContextSection {
    /// `SystemPrompt` | `History` | `ToolResults`.
    pub name: String,
    pub tokens: i64,
}

#[derive(Debug)]
pub struct SessionContextUsage {
    pub sections: Vec<SessionContextSection>,
    pub total_tokens: i64,
    pub budget_tokens: i64,
    pub prune_records: Vec<steward_usage::PruneRecord>,
}

impl From<steward_usage::StewardContextUsage> for SessionContextUsage {
    fn from(u: steward_usage::StewardContextUsage) -> Self {
        Self {
            sections: u
                .sections
                .into_iter()
                .map(|s| SessionContextSection {
                    name: s.name,
                    tokens: s.tokens,
                })
                .collect(),
            total_tokens: u.total_tokens,
            budget_tokens: u.budget_tokens,
            prune_records: u.prune_records,
        }
    }
}

/// What the DB lookup revealed about the session.
#[derive(Debug)]
enum SessionKind {
    Steward,
    Ordinary(Option<crate::services::ResolvedAgent>),
}

fn classify_session(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<SessionKind, SessionContextError> {
    // Session must exist at all.
    sessions::get(conn, session_id)?;
    if steward_repo::get_steward_session(conn, session_id).is_ok() {
        return Ok(SessionKind::Steward);
    }
    let participants = resolve_participants(conn, session_id)?;
    Ok(SessionKind::Ordinary(participants.into_iter().next()))
}

/// Optimizes a composer draft for any session. Steward sessions delegate
/// to the steward optimizer (bound provider, steward rewrite prompt);
/// ordinary sessions run the neutral rewrite on the session's materialized
/// provider. Every failure path returns an error — the draft is never
/// touched by this side.
pub async fn optimize_session_prompt(
    db_path: Arc<str>,
    secrets: Arc<dyn SecretStore>,
    _cwd: Option<PathBuf>,
    session_id: &str,
    draft: &str,
) -> Result<String, OptimizeError> {
    if draft.trim().is_empty() {
        return Err(OptimizeError::Failed("草稿为空".into()));
    }

    let kind = {
        let path = Arc::clone(&db_path);
        let sid = session_id.to_string();
        spawn_blocking(move || -> Result<SessionKind, SessionContextError> {
            let db = Db::open(&path)?;
            classify_session(&db.0, &sid)
        })
        .await
        .map_err(|e| OptimizeError::Failed(format!("join error: {e}")))?
        .map_err(|e| OptimizeError::Failed(e.to_string()))?
    };

    match kind {
        SessionKind::Steward => {
            prompt_optimizer::optimize_prompt(db_path, secrets, session_id, draft).await
        }
        SessionKind::Ordinary(_) => {
            // chat_rich_bind 5.6.1-2 (D9): ordinary sessions no longer
            // materialize the session's own provider — the steward's bound
            // model is the single optimization source for BOTH session
            // kinds. Unbound surfaces the dedicated `StewardUnbound` error
            // (IPC `prompt.steward_unbound`) so the frontend can route the
            // user to the steward-binding settings instead of a generic
            // failure toast.
            let binding = crate::steward::kernel::resolve_steward_binding(&db_path, secrets)
                .await
                .map_err(|e| OptimizeError::Failed(e.to_string()))?
                .ok_or(OptimizeError::StewardUnbound)?;
            prompt_optimizer::optimize_draft_with_provider(binding.provider, &binding.model, draft)
                .await
        }
    }
}

/// Aggregates the context usage of any session. Steward sessions delegate
/// to the steward aggregation (per-section + prune records); ordinary
/// sessions estimate from the session event log — `message` events form
/// History, `tool_result` events form ToolResults, and the first Role
/// participant's `system_prompt_override` forms SystemPrompt.
pub async fn get_session_context_usage(
    db_path: Arc<str>,
    session_id: &str,
) -> Result<SessionContextUsage, SessionContextError> {
    let sid = session_id.to_string();
    let (kind, events, system_prompt_len) = {
        let path = Arc::clone(&db_path);
        spawn_blocking(move || -> Result<(SessionKind, Vec<crate::domain::EventRecord>, usize), SessionContextError> {
            let db = Db::open(&path)?;
            let conn = &db.0;
            let kind = classify_session(conn, &sid)?;
            if matches!(kind, SessionKind::Steward) {
                return Ok((kind, Vec::new(), 0));
            }
            let events = events_repo::list_by_aggregate(conn, "session", &sid, None)?;
            // SystemPrompt length: first Role participant's override.
            let system_len = match &kind {
                SessionKind::Ordinary(Some(agent))
                    if matches!(agent.kind, crate::domain::AgentRefKind::Role) =>
                {
                    roles::get(conn, &agent.id)
                        .ok()
                        .and_then(|r| r.system_prompt_override)
                        .map(|s| s.len())
                        .unwrap_or(0)
                }
                _ => 0,
            };
            Ok((kind, events, system_len))
        })
        .await
        .map_err(|e| SessionContextError::Store(format!("join error: {e}")))?
    }?;

    match kind {
        SessionKind::Steward => {
            let usage = steward_usage::get_context_usage(db_path, session_id).await?;
            Ok(SessionContextUsage::from(usage))
        }
        SessionKind::Ordinary { .. } => {
            let mut history_len = 0usize;
            let mut tool_len = 0usize;
            for e in events.iter().take(EVENT_SCAN_LIMIT) {
                let content_len = e
                    .payload
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(|s| s.len())
                    .unwrap_or(0);
                match e.kind.as_str() {
                    "message" => history_len += content_len,
                    "tool_result" => tool_len += content_len,
                    _ => {}
                }
            }
            let sections = vec![
                SessionContextSection {
                    name: "SystemPrompt".into(),
                    tokens: approx_tokens_len(system_prompt_len),
                },
                SessionContextSection {
                    name: "History".into(),
                    tokens: approx_tokens_len(history_len),
                },
                SessionContextSection {
                    name: "ToolResults".into(),
                    tokens: approx_tokens_len(tool_len),
                },
            ];
            let total_tokens: i64 = sections.iter().map(|s| s.tokens).sum();
            Ok(SessionContextUsage {
                sections,
                total_tokens,
                budget_tokens: CONTEXT_BUDGET_TOKENS as i64,
                prune_records: Vec::new(),
            })
        }
    }
}

fn approx_tokens_len(len: usize) -> i64 {
    (len / 4) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AgentRefKind, ConversationKind};
    use crate::services::conversation_service::create_conversation;
    use crate::store::migrations;
    use serde_json::json;

    async fn db_path() -> Arc<str> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sess_ctx.db");
        let path_str = path.to_string_lossy().to_string();
        let db = Db::open(&path_str).unwrap();
        migrations::run(&db.0).unwrap();
        drop(db);
        std::mem::forget(dir);
        Arc::from(path_str)
    }

    fn seed_steward_session(conn: &rusqlite::Connection, id: &str) {
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at, kind)
             VALUES (?1, 'test', 1, 1, 'background')",
            [id],
        )
        .unwrap();
        steward_repo::insert_steward_session(
            conn,
            &crate::domain::StewardSession {
                id: id.into(),
                steward_id: "steward".into(),
                title: "test".into(),
                goal: None,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
    }

    fn seed_ordinary_session(conn: &rusqlite::Connection, role_override: Option<&str>) -> String {
        let role = crate::domain::Role {
            id: "r1".into(),
            name: "r1".into(),
            provider_id: None,
            provider_ids: vec![],
            system_prompt_override: role_override.map(str::to_string),
            tool_allowlist: vec![],
            required_capabilities: vec![],
            temperature: None,
            max_tokens: None,
            params: json!({}),
            builtin: false,
            generated: false,
            ephemeral: false,
            source: None,
            created_at: 1,
            updated_at: 1,
        };
        roles::insert(conn, &role).unwrap();
        let session = create_conversation(
            conn,
            ConversationKind::Chat,
            "test",
            &[(AgentRefKind::Role, "r1")],
            None,
            None,
            "__migrated__",
            None,
        )
        .unwrap();
        session.id
    }

    #[tokio::test]
    async fn unknown_session_usage_is_not_found() {
        let dbp = db_path().await;
        let err = get_session_context_usage(dbp, "nope").await.unwrap_err();
        assert!(matches!(err, SessionContextError::NotFound(_)));
    }

    #[tokio::test]
    async fn steward_session_delegates_to_steward_aggregation() {
        let dbp = db_path().await;
        {
            let db = Db::open(&dbp).unwrap();
            seed_steward_session(&db.0, "st-1");
        }
        let usage = get_session_context_usage(dbp, "st-1").await.unwrap();
        // Steward sections carry the base prompt; delegation is observable
        // via the SystemPrompt section being the base prompt length.
        assert_eq!(
            usage
                .sections
                .iter()
                .find(|s| s.name == "SystemPrompt")
                .map(|s| s.tokens),
            Some((crate::steward::llm_loop::STEWARD_BASE_PROMPT.len() / 4) as i64),
        );
        assert_eq!(usage.budget_tokens, CONTEXT_BUDGET_TOKENS as i64);
        assert!(usage.prune_records.is_empty());
    }

    #[tokio::test]
    async fn ordinary_session_aggregates_event_sections() {
        let dbp = db_path().await;
        let sid = {
            let db = Db::open(&dbp).unwrap();
            let sid = seed_ordinary_session(&db.0, Some("You are a coder."));
            events_repo::append(
                &db.0,
                "session",
                &sid,
                "message",
                &json!({"role": "user", "content": "hello world!"}),
                1,
            )
            .unwrap();
            events_repo::append(
                &db.0,
                "session",
                &sid,
                "message",
                &json!({"role": "assistant", "content": "hi there, coder"}),
                2,
            )
            .unwrap();
            events_repo::append(
                &db.0,
                "session",
                &sid,
                "tool_result",
                &json!({"call_id": "c1", "content": "x".repeat(40)}),
                3,
            )
            .unwrap();
            // Unrelated kinds must not count.
            events_repo::append(
                &db.0,
                "session",
                &sid,
                "thought",
                &json!({"content": "y".repeat(100)}),
                4,
            )
            .unwrap();
            sid
        };

        let usage = get_session_context_usage(dbp, &sid).await.unwrap();
        let by = |name: &str| -> i64 {
            usage
                .sections
                .iter()
                .find(|s| s.name == name)
                .map(|s| s.tokens)
                .unwrap()
        };
        assert_eq!(by("SystemPrompt"), ("You are a coder.".len() / 4) as i64);
        let expected_history = (("hello world!".len() + "hi there, coder".len()) / 4) as i64;
        assert_eq!(by("History"), expected_history);
        assert_eq!(by("ToolResults"), (40 / 4) as i64);
        assert_eq!(
            usage.total_tokens,
            by("SystemPrompt") + by("History") + by("ToolResults")
        );
        assert_eq!(usage.budget_tokens, CONTEXT_BUDGET_TOKENS as i64);
        assert!(usage.prune_records.is_empty());
    }

    #[tokio::test]
    async fn ordinary_session_without_role_override_counts_zero_system_prompt() {
        let dbp = db_path().await;
        let sid = {
            let db = Db::open(&dbp).unwrap();
            seed_ordinary_session(&db.0, None)
        };
        let usage = get_session_context_usage(dbp, &sid).await.unwrap();
        assert_eq!(
            usage
                .sections
                .iter()
                .find(|s| s.name == "SystemPrompt")
                .map(|s| s.tokens),
            Some(0),
        );
        assert_eq!(usage.total_tokens, 0);
    }

    #[tokio::test]
    async fn optimize_unknown_session_fails() {
        let (dbp, secrets) = (
            db_path().await,
            Arc::new(crate::providers::MemorySecretStore::default()) as Arc<dyn SecretStore>,
        );
        let err = optimize_session_prompt(dbp, secrets, None, "nope", "帮我看看")
            .await
            .unwrap_err();
        assert!(matches!(err, OptimizeError::Failed(_)));
    }

    #[tokio::test]
    async fn optimize_steward_session_delegates() {
        let (dbp, secrets) = (
            db_path().await,
            Arc::new(crate::providers::MemorySecretStore::default()) as Arc<dyn SecretStore>,
        );
        {
            let db = Db::open(&dbp).unwrap();
            seed_steward_session(&db.0, "st-2");
        }
        // No steward binding seeded → the steward optimizer's binding hint
        // proves the delegation path was taken.
        let err = optimize_session_prompt(dbp, secrets, None, "st-2", "帮我诊断")
            .await
            .unwrap_err();
        match err {
            OptimizeError::Failed(msg) => assert!(msg.contains("绑定")),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn optimize_ordinary_session_without_provider_fails() {
        let (dbp, secrets) = (
            db_path().await,
            Arc::new(crate::providers::MemorySecretStore::default()) as Arc<dyn SecretStore>,
        );
        let sid = {
            let db = Db::open(&dbp).unwrap();
            seed_ordinary_session(&db.0, None)
        };
        let err = optimize_session_prompt(dbp, secrets, None, &sid, "帮我看看")
            .await
            .unwrap_err();
        // chat_rich_bind 5.6.1-2 (D9): the steward's bound model is the
        // single optimization source; no binding → the dedicated
        // StewardUnbound code (IPC `prompt.steward_unbound`).
        assert!(matches!(err, OptimizeError::StewardUnbound));
    }

    #[tokio::test]
    async fn optimize_empty_draft_fails_before_db_work() {
        let (dbp, secrets) = (
            db_path().await,
            Arc::new(crate::providers::MemorySecretStore::default()) as Arc<dyn SecretStore>,
        );
        let err = optimize_session_prompt(dbp, secrets, None, "whatever", "   ")
            .await
            .unwrap_err();
        assert!(matches!(err, OptimizeError::Failed(_)));
    }
}
