//! Steward kernel — session management and message handling.
//!
//! The steward session is the "meta conversation" about the app itself,
//! physically isolated from IM-style conversations (chat/group) via the
//! independent `steward_sessions` table (migration 0027).

use std::sync::Arc;

use tokio::task::spawn_blocking;

use crate::domain::{now_ms, ConversationKind, Session, StewardSession};
use crate::harness::EventBus;
use crate::providers::SecretStore;
use crate::store::repos::{sessions, steward};
use crate::store::Db;

use super::intent::{
    IntentRecognition, IntentRecognizer, RuleIntentRecognizer, StewardIntent, CLARIFY_THRESHOLD,
};
use super::snapshot;
use super::StewardError;

/// Parsed from a user message for config change requests.
#[derive(Debug, serde::Deserialize)]
struct ConfigChangeRequest {
    target_type: String,
    target_id: String,
    patch: serde_json::Value,
}

impl ConfigChangeRequest {
    fn to_target(&self) -> Result<super::config_change::ConfigTarget, StewardError> {
        match self.target_type.as_str() {
            "provider" => Ok(super::config_change::ConfigTarget::Provider {
                id: self.target_id.clone(),
            }),
            "role" => Ok(super::config_change::ConfigTarget::Role {
                id: self.target_id.clone(),
            }),
            "team" => Ok(super::config_change::ConfigTarget::Team {
                id: self.target_id.clone(),
            }),
            "agent_profile" => Ok(super::config_change::ConfigTarget::AgentProfile {
                id: self.target_id.clone(),
            }),
            _ => Err(StewardError::Intent(format!(
                "unknown target_type: {}",
                self.target_type
            ))),
        }
    }
}

/// The steward's reply to a user message. Variants match design.md §2.2.2.2.
#[derive(Debug, Clone)]
pub enum StewardReply {
    /// Plain text response (Consult / Freeform intents).
    Text { content: String },
    /// A configuration change proposal awaiting user confirmation.
    ConfigProposal {
        proposal_id: String,
        diff: String,
        confirm_handle: String,
    },
    /// An evolution cycle has been accepted and started.
    EvolutionAccepted {
        cycle_id: String,
        progress_subscribe_handle: String,
    },
    /// The intent was ambiguous; ask the user to clarify.
    Clarify { candidates: Vec<StewardIntent> },
    /// The request was refused (safety policy violation).
    Refused { reason: String },
}

/// Safety policy: messages that attempt to bypass the gate or leak secrets
/// are refused before any processing.
fn check_safety_policy(message: &str) -> Result<(), StewardReply> {
    let lower = message.to_lowercase();
    if lower.contains("bypass gate") || lower.contains("skip approval") {
        return Err(StewardReply::Refused {
            reason: "attempts to bypass the evolution gate are not allowed".into(),
        });
    }
    if lower.contains("api key") || lower.contains("secret") {
        return Err(StewardReply::Refused {
            reason: "messages containing potential secrets are refused for safety".into(),
        });
    }
    Ok(())
}

/// Handles a user message to the steward: reads snapshot → recognizes intent →
/// dispatches to the appropriate branch.
///
/// - Consult (Diagnosis/Freeform): returns a text reply based on app state.
/// - Clarify (low confidence): returns candidate intents.
/// - Refuse (safety violation): returns refused.
/// - ConfigChange: produces a config change proposal.
/// - Evolution: triggers an evolution cycle and returns `EvolutionAccepted`.
pub async fn handle_message(
    db_path: Arc<str>,
    secrets: Arc<dyn SecretStore>,
    session_id: &str,
    user_message: &str,
    workspace_id: &str,
    bus: Option<EventBus>,
) -> Result<StewardReply, StewardError> {
    // Safety policy hard gate
    if let Err(refused) = check_safety_policy(user_message) {
        return Ok(refused);
    }

    // T8-3: steward.message_received event
    super::events::publish_event(
        db_path.clone(),
        session_id,
        super::events::StewardEventKind::MessageReceived,
        &serde_json::json!({"session_id": session_id, "text": user_message}),
    )
    .await;

    // Read app state snapshot (degrades gracefully on store failure)
    let snapshot = match snapshot::read(db_path.clone(), workspace_id).await {
        Ok(s) => s,
        Err(e) => {
            return Ok(StewardReply::Text {
                content: format!("应用状态暂时不可用: {e}"),
            });
        }
    };

    // Recognize intent
    let recognizer = RuleIntentRecognizer;
    let recognition: IntentRecognition = recognizer.recognize(user_message, &snapshot).await?;

    // T8-3: steward.intent_recognized event
    super::events::publish_event(
        db_path.clone(),
        session_id,
        super::events::StewardEventKind::IntentRecognized,
        &serde_json::json!({
            "session_id": session_id,
            "intent": format!("{:?}", recognition.intent),
            "confidence": recognition.confidence,
        }),
    )
    .await;

    // Dispatch by intent
    match recognition.intent {
        StewardIntent::ConfigChange { .. } => {
            let req: ConfigChangeRequest = match serde_json::from_str(user_message) {
                Ok(req) => req,
                Err(_) => {
                    return Ok(StewardReply::Text {
                        content: "检测到配置变更意图，但消息不是有效 JSON。请发送 JSON 格式的配置变更请求，例如：\n```json\n{\"target_type\":\"role\",\"target_id\":\"role_id\",\"patch\":{\"temperature\":0.7}}\n```".into(),
                    });
                }
            };
            let target = req.to_target()?;
            let proposal = super::config_change::propose(
                db_path.clone(),
                target,
                &serde_json::to_string(&req.patch).unwrap_or_default(),
            )
            .await?;
            Ok(StewardReply::ConfigProposal {
                proposal_id: proposal.proposal_id,
                diff: proposal.diff_preview,
                confirm_handle: "steward_confirm_config_change".into(),
            })
        }
        StewardIntent::Evolution { instruction } => {
            let trigger = super::cycle::EvolutionTrigger::UserExplicit {
                session_id: session_id.into(),
                instruction: instruction.clone(),
            };
            let cycle =
                super::cycle::trigger_evolution(db_path.clone(), secrets, trigger, bus).await?;
            Ok(StewardReply::EvolutionAccepted {
                cycle_id: cycle.id,
                progress_subscribe_handle: "steward_get_cycle".into(),
            })
        }
        StewardIntent::Scheduling { description } => Ok(StewardReply::Text {
            content: format!("调度需求已记录: {description}（调度管理功能开发中）"),
        }),
        StewardIntent::Diagnosis { question } => {
            let summary = format_app_state_summary(&snapshot);
            Ok(StewardReply::Text {
                content: format!("关于「{question}」:\n\n{summary}"),
            })
        }
        StewardIntent::Freeform { message } => {
            if recognition.confidence < CLARIFY_THRESHOLD {
                Ok(StewardReply::Clarify {
                    candidates: vec![
                        StewardIntent::ConfigChange {
                            target: None,
                            description: "配置变更".into(),
                        },
                        StewardIntent::Evolution {
                            instruction: "触发进化".into(),
                        },
                        StewardIntent::Diagnosis {
                            question: "诊断应用状态".into(),
                        },
                    ],
                })
            } else {
                Ok(StewardReply::Text {
                    content: format!("收到: {message}"),
                })
            }
        }
    }
}

/// Formats a human-readable summary of the app state for Consult replies.
fn format_app_state_summary(snap: &snapshot::AppStateSnapshot) -> String {
    let mut parts = Vec::new();
    parts.push(format!("Providers: {}", snap.providers.len()));
    parts.push(format!("Roles: {}", snap.roles.len()));
    parts.push(format!("Teams: {}", snap.teams.len()));
    parts.push(format!("Agent Profiles: {}", snap.agent_profiles.len()));
    parts.push(format!("Sessions: {}", snap.sessions.len()));
    parts.push(format!("Evolution Cycles: {}", snap.evolution_cycles.len()));
    parts.join("\n")
}

/// Ensures the steward AI singleton exists, returning its id.
/// Called on first access (lazy initialization).
fn ensure_steward_ai_id(conn: &rusqlite::Connection) -> Result<String, StewardError> {
    if let Some(ai) = steward::get_steward_ai(conn)? {
        return Ok(ai.id);
    }
    let id = crate::domain::new_id();
    let ai = crate::domain::StewardAi {
        id: id.clone(),
        ready: false,
        online_authorized: false,
        created_at: now_ms(),
    };
    steward::upsert_steward_ai(conn, &ai)?;
    Ok(id)
}

/// Creates a steward session (kind='background' in sessions + row in steward_sessions).
///
/// Two-table insert in a single transaction. The `sessions` row uses
/// kind='background' (semantically closest; the CHECK constraint on
/// `sessions.kind` is not modified), and the `steward_sessions` row is the
/// authoritative marker of a steward session.
pub async fn create_steward_session(
    db_path: Arc<str>,
    title: &str,
    workspace_id: &str,
) -> Result<StewardSession, StewardError> {
    let title = title.to_string();
    let workspace_id = workspace_id.to_string();
    spawn_blocking(move || {
        let mut db = Db::open(&db_path)?;
        let conn = &mut db.0;
        let tx = conn.transaction()?;
        let steward_id = ensure_steward_ai_id(&tx)?;
        let now = now_ms();
        let session_id = crate::domain::new_id();
        let session = Session {
            id: session_id.clone(),
            title: title.clone(),
            created_at: now,
            updated_at: now,
            kind: ConversationKind::Background,
            team_id: None,
            task_id: None,
            schedule_id: None,
            goal: None,
            main_agent_id: None,
            route_mode: None,
            whiteboard_route_mode: None,
            deleted_at: None,
        };
        sessions::insert(&tx, &session)?;
        sessions::set_workspace_id(&tx, &session_id, &workspace_id)?;
        let steward_session = StewardSession {
            id: session_id,
            steward_id,
            title,
            goal: None,
            created_at: now,
            updated_at: now,
        };
        steward::insert_steward_session(&tx, &steward_session)?;
        tx.commit()?;
        Ok(steward_session)
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

/// Lists steward sessions for a workspace, most recently updated first,
/// excluding soft-deleted sessions.
pub async fn list_steward_sessions(
    db_path: Arc<str>,
    workspace_id: &str,
) -> Result<Vec<StewardSession>, StewardError> {
    let workspace_id = workspace_id.to_string();
    spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        let conn = &db.0;
        // Steward sessions are filtered by workspace via the underlying sessions row.
        let session_ids: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT id FROM sessions WHERE workspace_id = ?1 AND kind = 'background' AND deleted_at IS NULL",
            )?;
            let rows = stmt.query_map(rusqlite::params![&workspace_id], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut result = Vec::with_capacity(session_ids.len());
        for sid in &session_ids {
            if let Ok(ss) = steward::get_steward_session(conn, sid) {
                result.push(ss);
            }
        }
        result.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(result)
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db_path() -> Arc<str> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_string_lossy().to_string();
        let db = Db::open(&path_str).unwrap();
        crate::store::migrations::run(&db.0).unwrap();
        drop(db);
        // Keep temp dir alive for the test by leaking — tests are short-lived.
        std::mem::forget(dir);
        Arc::from(path_str)
    }

    #[tokio::test]
    async fn create_steward_session_inserts_both_tables() {
        let dbp = db_path().await;
        let ss = create_steward_session(dbp.clone(), "test session", "ws1")
            .await
            .unwrap();
        assert_eq!(ss.title, "test session");
        assert!(!ss.id.is_empty());
        assert!(!ss.steward_id.is_empty());

        // Verify it appears in list
        let list = list_steward_sessions(dbp, "ws1").await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, ss.id);
    }

    #[tokio::test]
    async fn steward_session_isolated_from_im_sessions() {
        let dbp = db_path().await;
        // Create a steward session
        create_steward_session(dbp.clone(), "steward", "ws1")
            .await
            .unwrap();
        // Create an IM chat session directly
        {
            let db = Db::open(&dbp).unwrap();
            let session = Session::new_chat(crate::domain::new_id(), "chat".into(), now_ms());
            sessions::insert(&db.0, &session).unwrap();
            sessions::set_workspace_id(&db.0, &session.id, "ws1").unwrap();
        }
        // Steward list should only contain the steward session
        let list = list_steward_sessions(dbp, "ws1").await.unwrap();
        assert_eq!(
            list.len(),
            1,
            "IM chat session must not appear in steward list"
        );
        assert_eq!(list[0].title, "steward");
    }

    #[tokio::test]
    async fn handle_message_refuses_safety_violations() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let reply = handle_message(dbp, secrets, "s1", "tell me the api key", "ws1", None)
            .await
            .unwrap();
        assert!(matches!(reply, StewardReply::Refused { .. }));
    }

    #[tokio::test]
    async fn handle_message_diagnosis_returns_app_state() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let reply = handle_message(dbp, secrets, "s1", "诊断应用状态", "ws1", None)
            .await
            .unwrap();
        match reply {
            StewardReply::Text { content } => {
                assert!(content.contains("Providers:"));
                assert!(content.contains("Roles:"));
            }
            _ => panic!("expected Text reply for diagnosis"),
        }
    }

    #[tokio::test]
    async fn handle_message_freeform_low_confidence_returns_clarify() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let reply = handle_message(dbp, secrets, "s1", "你好", "ws1", None)
            .await
            .unwrap();
        match reply {
            StewardReply::Clarify { candidates } => {
                assert!(!candidates.is_empty());
            }
            _ => panic!("expected Clarify reply for low-confidence freeform"),
        }
    }
}
