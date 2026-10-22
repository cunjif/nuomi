//! Steward kernel — session management and message handling.
//!
//! The steward session is the "meta conversation" about the app itself,
//! physically isolated from IM-style conversations (chat/group) via the
//! independent `steward_sessions` table (migration 0027).

use std::sync::Arc;

use rusqlite::OptionalExtension;
use tokio::task::spawn_blocking;

use crate::domain::{now_ms, ConversationKind, Session, StewardSession};
use crate::harness::EventBus;
use crate::providers::{ChatMessage, LlmProvider, SecretStore};
use crate::store::repos::{sessions, steward};
use crate::store::Db;

use super::intent::{IntentRecognizer, RuleIntentRecognizer, StewardIntent};
use super::llm_loop::{self, StewardLoopInput};
use super::snapshot;
use super::StewardError;

/// The steward's reply to a user message. Variants match design.md §2.2.2.2.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
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

/// Role of a steward message — either the user or the steward itself.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StewardMessageRole {
    User,
    Steward,
}

/// A persisted steward message — used for history recovery on page reload.
///
/// Messages are reconstructed from `steward.message_received` (user) and
/// `steward.message_sent` (steward) events in the append-only `events` table.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StewardMessage {
    pub id: String,
    pub role: StewardMessageRole,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<StewardReply>,
    pub created_at: i64,
}

/// Safety policy: messages that attempt to bypass the gate or leak secrets
/// are refused before any processing. Shared with the agent loop
/// (`llm_loop::run`'s SafetyGate state).
pub(crate) fn check_safety_policy(message: &str) -> Result<(), StewardReply> {
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

/// Guidance shown when the steward has no bound model service. The rule
/// intent recognizer's remaining scope: tailoring this hint (steward_workbench
/// v2 — the recognizer no longer drives the reply path).
const BINDING_GUIDANCE: &str = "管家尚未绑定可用的模型服务，暂时无法对话。请前往「管家设置 → Provider 绑定」绑定一个 Provider（并确认其已配置模型，或指定 ModelId）或 CLI Agent 后再试。";

/// Maximum prior turns loaded into the loop context (the pruning pipeline
/// enforces the actual token budget).
pub(crate) const HISTORY_LIMIT: u32 = 100;

/// The steward's bound model service, resolved from the steward main role.
pub(crate) struct StewardBinding {
    pub(crate) provider: Arc<dyn LlmProvider>,
    pub(crate) model: String,
    /// SOUL persona — injected as the first paragraph of the system prompt.
    pub(crate) soul: Option<String>,
    pub(crate) temperature: Option<f64>,
    /// Whether the bound provider carries the `image` capability tag
    /// (drives the multimodal-vs-placeholder split, design §2.1.3.2).
    pub(crate) supports_vision: bool,
}

/// Resolves the steward main role's binding into a ready provider.
///
/// `None` (unbound or dangling binding) means the caller should return the
/// binding guidance instead of running the LLM loop — never a clarify reply.
pub(crate) async fn resolve_steward_binding(
    db_path: &Arc<str>,
    secrets: Arc<dyn SecretStore>,
) -> Result<Option<StewardBinding>, StewardError> {
    let role = {
        let path = Arc::clone(db_path);
        spawn_blocking(
            move || -> Result<Option<crate::domain::Role>, StewardError> {
                let db = Db::open(&path)?;
                Ok(
                    crate::store::repos::roles::get(&db.0, super::settings::STEWARD_MAIN_ROLE_ID)
                        .ok(),
                )
            },
        )
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))?
    }?;
    let Some(role) = role else {
        return Ok(None);
    };
    if super::settings::infer_binding_mode(&role) == super::settings::BindingMode::None {
        return Ok(None);
    }
    let materialized =
        crate::services::team_runner::materialize(Arc::clone(db_path), secrets, None)
            .await
            .map_err(|e| StewardError::Store(e.to_string()))?;
    let bound_provider_id = role.provider_id.clone();
    let key = super::settings::read_agent_profile_id(&role.params).or(role.provider_id);
    let provider = key
        .as_deref()
        .and_then(|k| materialized.providers.get(k).cloned());
    let Some(provider) = provider else {
        // Bound target missing (deleted provider / disabled profile) — treat
        // as unbound and guide the user back to settings.
        return Ok(None);
    };
    // The wire model id (the loop's ChatRequest.model):
    // 1. the steward role's explicit `params.model_id` pin,
    // 2. the bound binding's resolved model (provider `settings.default_model`
    //    > first configured model; CLI profiles carry their own `model_id`).
    // A Provider binding that resolves no model at all is treated as
    // unbound — sending a placeholder model id to the wire would only
    // produce a guaranteed-rejected request. CLI bindings never need a
    // model id (the CLI client ignores ChatRequest.model), so they fall
    // back to the client tag for display purposes.
    let model = match (
        &bound_provider_id,
        super::settings::read_model_id(&role.params),
    ) {
        (Some(_), pinned) => pinned.or_else(|| {
            bound_provider_id
                .as_deref()
                .and_then(|pid| materialized.models.get(pid).cloned())
        }),
        (None, _) => Some(
            key.as_deref()
                .and_then(|k| materialized.models.get(k).cloned())
                .unwrap_or_else(|| provider.id().to_string()),
        ),
    };
    let Some(model) = model else {
        // Provider bound but no model knowable (settings.default_model and
        // settings.models both empty) — same guidance path as unbound.
        return Ok(None);
    };
    // Vision capability: only a directly bound ProviderConfig can carry the
    // `image` tag (CLI-agent bindings expose no capability metadata — treat
    // as non-vision so images degrade to descriptive placeholders).
    let supports_vision = if let Some(pid) = bound_provider_id.as_deref() {
        let path = Arc::clone(db_path);
        let pid = pid.to_string();
        spawn_blocking(move || -> bool {
            let Ok(db) = Db::open(&path) else {
                return false;
            };
            crate::store::repos::providers::get_provider(&db.0, &pid)
                .map(|cfg| {
                    cfg.capability_union()
                        .iter()
                        .any(|c| matches!(c, crate::domain::Capability::Image))
                })
                .unwrap_or(false)
        })
        .await
        .unwrap_or(false)
    } else {
        false
    };
    Ok(Some(StewardBinding {
        model,
        provider,
        soul: role.system_prompt_override,
        temperature: role.temperature,
        supports_vision,
    }))
}

/// Tailors the binding guidance with a rule-recognized intent hint (the
/// recognizer's only remaining production use).
async fn binding_guidance_reply(
    db_path: Arc<str>,
    user_message: &str,
    workspace_id: &str,
) -> StewardReply {
    let mut content = BINDING_GUIDANCE.to_string();
    if let Ok(snap) = snapshot::read(db_path, workspace_id).await {
        if let Ok(recognition) = RuleIntentRecognizer.recognize(user_message, &snap).await {
            content.push('\n');
            content.push_str(intent_hint(&recognition.intent));
        }
    }
    StewardReply::Text { content }
}

/// One-line capability hint per intent class, used by the binding guidance.
fn intent_hint(intent: &StewardIntent) -> &'static str {
    match intent {
        StewardIntent::ConfigChange { .. } => {
            "绑定后将可为您分析并提议配置变更（含 diff 预览，经确认后生效）。"
        }
        StewardIntent::Evolution { .. } => "绑定后将可为您触发自我进化周期。",
        StewardIntent::Scheduling { .. } => "绑定后将可为您查看与管理定时调度。",
        StewardIntent::Diagnosis { .. } => "绑定后将可基于实时应用状态为您诊断。",
        StewardIntent::Freeform { .. } => "绑定后即可开始自然对话。",
    }
}

/// Loads the steward conversation history as provider-layer messages.
async fn steward_history(
    db_path: Arc<str>,
    session_id: &str,
) -> Result<Vec<ChatMessage>, StewardError> {
    let messages = list_session_messages(db_path, session_id, HISTORY_LIMIT).await?;
    Ok(messages
        .into_iter()
        .map(|m| match m.role {
            StewardMessageRole::User => ChatMessage::user(m.text),
            StewardMessageRole::Steward => ChatMessage::assistant(m.text),
        })
        .collect())
}

/// Handles a user message to the steward: safety gate → binding check →
/// attachment context assembly → LLM agent loop (design §2.1.3.1/§2.1.3.2).
///
/// - Unbound: returns the binding guidance (never a clarify reply).
/// - Bound: runs the agent loop — natural conversation over the session
///   history with the steward tools (config proposals, evolution, state).
/// - Safety violations are refused before any LLM traffic.
/// - Attachments are validated before any event is recorded: an unusable
///   attachment aborts the send (`AttachmentInvalid` names the file), so
///   the message is never persisted as received.
#[allow(clippy::too_many_arguments)]
pub async fn handle_message(
    db_path: Arc<str>,
    secrets: Arc<dyn SecretStore>,
    session_id: &str,
    user_message: &str,
    workspace_id: &str,
    workspace_root: Option<std::path::PathBuf>,
    attachment_ids: &[String],
    bus: Option<EventBus>,
) -> Result<StewardReply, StewardError> {
    // Safety policy hard gate — refuse before any LLM traffic, but audit
    // the refusal first (spec 5.1.3-3b: "直接返回拒绝回复，不进入 LLM，落审计事件").
    if let Err(refused) = check_safety_policy(user_message) {
        let refusal_reason = match &refused {
            StewardReply::Refused { reason } => reason.clone(),
            other => reply_text_for_event(other),
        };
        super::events::publish_event(
            db_path.clone(),
            session_id,
            super::events::StewardEventKind::SafetyRefused,
            &serde_json::json!({
                "session_id": session_id,
                "reason": refusal_reason,
            }),
        )
        .await;
        return Ok(refused);
    }

    // BindingCheck first: the vision split of the attachment assembly below
    // depends on the bound provider's capability tags.
    enum Turn {
        Guidance,
        Loop(StewardBinding, super::context_builder::AssembledAttachments),
    }
    let turn = match resolve_steward_binding(&db_path, Arc::clone(&secrets)).await? {
        None => Turn::Guidance,
        Some(binding) => {
            // Attachment context (design §2.1.3.2): validate + assemble
            // before the message is recorded — an unusable attachment aborts
            // the send naming the file, leaving no trace events behind.
            let attachments = super::context_builder::build_attachment_context(
                db_path.clone(),
                workspace_root.unwrap_or_else(|| std::path::PathBuf::from(".")),
                session_id,
                attachment_ids,
                binding.supports_vision,
            )
            .await
            .map_err(|e| StewardError::AttachmentInvalid {
                name: e.name,
                reason: e.reason,
            })?;
            Turn::Loop(binding, attachments)
        }
    };

    // steward.message_received event
    super::events::publish_event(
        db_path.clone(),
        session_id,
        super::events::StewardEventKind::MessageReceived,
        &serde_json::json!({"session_id": session_id, "text": user_message}),
    )
    .await;

    let reply = match turn {
        Turn::Guidance => binding_guidance_reply(db_path.clone(), user_message, workspace_id).await,
        Turn::Loop(binding, attachments) => {
            let history = steward_history(db_path.clone(), session_id).await?;
            let input = StewardLoopInput {
                session_id: session_id.to_string(),
                user_message: user_message.to_string(),
                workspace_id: workspace_id.to_string(),
                history,
                attachments: attachments.text_blocks,
                image_parts: attachments.image_parts,
            };
            llm_loop::run(
                db_path.clone(),
                secrets,
                bus.clone(),
                binding.provider,
                &binding.model,
                binding.soul.as_deref(),
                binding.temperature,
                input,
            )
            .await?
        }
    };

    // Record steward reply for history recovery (steward.message_sent event).
    publish_message_sent(db_path.clone(), session_id, &reply).await;

    // Authoritative reply events (design §2.2.2-A1 postconditions):
    // `steward.message` carries the final authoritative text (the frontend
    // uses it to take over the streamed rendering), `steward.reply_completed`
    // closes the reply turn (full reply JSON).
    super::events::publish_event(
        db_path.clone(),
        session_id,
        super::events::StewardEventKind::Message,
        &serde_json::json!({
            "session_id": session_id,
            "text": reply_text_for_event(&reply),
        }),
    )
    .await;
    super::events::publish_event(
        db_path.clone(),
        session_id,
        super::events::StewardEventKind::ReplyCompleted,
        &serde_json::json!({
            "session_id": session_id,
            "reply": serde_json::to_value(&reply).unwrap_or(serde_json::json!({})),
        }),
    )
    .await;

    // Live mirror onto the EventBus (ADR-0002 session channel): the events
    // above are the durable authority; these bus copies drive the frontend's
    // streaming takeover (`steward.message`) and turn close
    // (`steward.reply_completed`). Payloads carry camelCase `sessionId` for
    // bridge routing to `event://session/{id}`.
    super::events::publish_bus(
        bus.as_ref(),
        super::events::StewardEventKind::Message.as_str(),
        serde_json::json!({
            "sessionId": session_id,
            "text": reply_text_for_event(&reply),
        }),
    );
    super::events::publish_bus(
        bus.as_ref(),
        super::events::StewardEventKind::ReplyCompleted.as_str(),
        serde_json::json!({
            "sessionId": session_id,
            "reply": serde_json::to_value(&reply).unwrap_or(serde_json::json!({})),
        }),
    );

    Ok(reply)
}

/// Best-effort publish of a `steward.message_sent` event for history recovery.
///
/// Uses `events::append` directly (bypasses `StewardEventKind` to avoid
/// modifying the 17-variant enum). Non-fatal on failure.
async fn publish_message_sent(db_path: Arc<str>, session_id: &str, reply: &StewardReply) {
    let session_id = session_id.to_string();
    let reply_text = reply_text_for_event(reply);
    let reply_json = serde_json::to_value(reply).unwrap_or(serde_json::json!({}));
    let payload = serde_json::json!({
        "session_id": session_id,
        "text": reply_text,
        "reply": reply_json,
    });
    let _ = tokio::task::spawn_blocking(move || {
        let db = crate::store::Db::open(&db_path)?;
        crate::store::repos::events::append(
            &db.0,
            "steward",
            &session_id,
            "steward.message_sent",
            &payload,
            crate::domain::now_ms(),
        )?;
        Ok::<_, crate::store::StoreError>(())
    })
    .await;
}

/// Extracts a plain-text summary of a reply for event logging.
fn reply_text_for_event(reply: &StewardReply) -> String {
    match reply {
        StewardReply::Text { content } => content.clone(),
        StewardReply::ConfigProposal {
            proposal_id, diff, ..
        } => format!("配置变更建议（{proposal_id}）：\n{diff}"),
        StewardReply::EvolutionAccepted { cycle_id, .. } => {
            format!("进化周期已受理，周期 ID: {cycle_id}")
        }
        StewardReply::Clarify { candidates } => {
            format!("请明确您的意图：{}", candidates.len())
        }
        StewardReply::Refused { reason } => format!("拒绝: {reason}"),
    }
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

/// Ensures the singleton steward session exists, returning the unique active
/// session. Idempotent: if sessions exist, returns the most recently updated
/// one; if none, creates a new one. Publishes a `steward.session_ready` event.
///
/// This is the authoritative implementation of the single-session constraint
/// (design.md §2.1.3). Historical sessions are preserved (not deleted) and
/// not exposed to the frontend (no switch entry).
pub async fn ensure_singleton_session(
    db_path: Arc<str>,
    workspace_id: &str,
) -> Result<StewardSession, StewardError> {
    let sessions = list_steward_sessions(db_path.clone(), workspace_id).await?;

    let (session, is_new, merged_from): (StewardSession, bool, Vec<String>) =
        if let Some(latest) = sessions.first() {
            let merged = if sessions.len() > 1 {
                sessions.iter().skip(1).map(|s| s.id.clone()).collect()
            } else {
                Vec::new()
            };
            (latest.clone(), false, merged)
        } else {
            let s = create_steward_session(db_path.clone(), "管家会话", workspace_id).await?;
            (s, true, Vec::new())
        };

    // Publish session_ready event (bypasses StewardEventKind to avoid
    // modifying the 17-variant enum). Best-effort, non-fatal.
    publish_session_ready(db_path, &session.id, is_new, &merged_from).await;

    Ok(session)
}

/// Best-effort publish of a `steward.session_ready` event.
async fn publish_session_ready(
    db_path: Arc<str>,
    session_id: &str,
    is_new: bool,
    merged_from: &[String],
) {
    let session_id = session_id.to_string();
    let merged_from = merged_from.to_vec();
    let payload = serde_json::json!({
        "session_id": session_id,
        "is_new": is_new,
        "merged_from": merged_from,
        "active_session_id": session_id,
    });
    let _ = tokio::task::spawn_blocking(move || {
        let db = crate::store::Db::open(&db_path)?;
        crate::store::repos::events::append(
            &db.0,
            "steward",
            &session_id,
            "steward.session_ready",
            &payload,
            crate::domain::now_ms(),
        )?;
        Ok::<_, crate::store::StoreError>(())
    })
    .await;
}

/// Lists messages for a steward session, ordered by `created_at` ascending.
///
/// Messages are reconstructed from `steward.message_received` (user) and
/// `steward.message_sent` (steward) events in the append-only `events` table.
/// Returns `StewardError::NotFound` if the session does not exist.
pub async fn list_session_messages(
    db_path: Arc<str>,
    session_id: &str,
    limit: u32,
) -> Result<Vec<StewardMessage>, StewardError> {
    let session_id = session_id.to_string();
    spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        let conn = &db.0;

        // Verify session exists
        if steward::get_steward_session(conn, &session_id).is_err() {
            return Err(StewardError::NotFound {
                entity: "steward_session",
                id: session_id.clone(),
            });
        }

        // Read cleared_at filter point (NULL = no filter)
        let cleared_at: Option<i64> = conn
            .query_row(
                "SELECT cleared_at FROM steward_sessions WHERE id = ?1",
                rusqlite::params![&session_id],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten();

        // Build query with cleared_at filter and deletion marker exclusion.
        // steward_deleted_events.event_id stores the numeric event id as TEXT,
        // so we CAST(e.id AS TEXT) for the join condition.
        // When cleared_at is NULL, use a very old timestamp so the condition is always true.
        let cleared_filter = cleared_at.unwrap_or(i64::MIN);

        let mut stmt = conn.prepare(
            "SELECT e.id, e.kind, e.payload, e.created_at FROM events e
             LEFT JOIN steward_deleted_events d
               ON d.session_id = ?1 AND d.event_id = CAST(e.id AS TEXT)
             WHERE e.aggregate_type = 'steward' AND e.aggregate_id = ?1
               AND e.kind IN ('steward.message_received', 'steward.message_sent')
               AND e.created_at > ?2
               AND d.event_id IS NULL
             ORDER BY e.created_at ASC, e.id ASC LIMIT ?3",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![&session_id, cleared_filter, limit],
            |row| {
                let id: i64 = row.get(0)?;
                let kind: String = row.get(1)?;
                let payload_str: String = row.get(2)?;
                let created_at: i64 = row.get(3)?;
                Ok((id, kind, payload_str, created_at))
            },
        )?;

        let mut messages = Vec::new();
        for row in rows {
            let (id, kind, payload_str, created_at) = row?;
            let payload: serde_json::Value = serde_json::from_str(&payload_str).unwrap_or_default();

            if kind == "steward.message_received" {
                let text = payload
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                messages.push(StewardMessage {
                    id: format!("evt-{id}"),
                    role: StewardMessageRole::User,
                    text,
                    reply: None,
                    created_at,
                });
            } else if kind == "steward.message_sent" {
                let text = payload
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let reply = payload
                    .get("reply")
                    .and_then(|v| serde_json::from_value(v.clone()).ok());
                messages.push(StewardMessage {
                    id: format!("evt-{id}"),
                    role: StewardMessageRole::Steward,
                    text,
                    reply,
                    created_at,
                });
            }
        }

        Ok(messages)
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
    async fn handle_message_unusable_attachment_aborts_without_events() {
        let dbp = db_path().await;
        let session = create_steward_session(dbp.clone(), "管家会话", "ws1")
            .await
            .unwrap();
        {
            let db = Db::open(&dbp).unwrap();
            db.0
                .execute(
                    "INSERT INTO provider_configs (id, name, protocol, base_url, keyring_ref, params_json, created_at, updated_at)
                     VALUES ('p1', 'Bound', 'openai_compatible', 'http://127.0.0.1:9', 'kr/p1', ?1, 1, 1)",
                    [r#"{"settings":{"models":[{"id":"gpt-4o-mini","capabilities":["reasoning"]}]}}"#],
                )
                .unwrap();
            db.0.execute(
                "UPDATE roles SET provider_id = 'p1' WHERE id = 'role_steward_main'",
                [],
            )
            .unwrap();
        }
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        secrets.set("kr/p1", "sk-test").await.unwrap();
        let err = handle_message(
            dbp.clone(),
            secrets,
            &session.id,
            "总结这个文档",
            "ws1",
            None,
            &["missing-attachment".to_string()],
            None,
        )
        .await
        .unwrap_err();
        match err {
            StewardError::AttachmentInvalid { name, reason } => {
                assert_eq!(name, "missing-attachment");
                assert!(reason.contains("不属于当前会话"), "got: {reason}");
            }
            other => panic!("expected AttachmentInvalid, got {other:?}"),
        }
        // Nothing was recorded — the send never happened (design §2.1.3.2).
        let db = Db::open(&dbp).unwrap();
        let evts = super::super::events::list(&db.0, Some(&session.id), None, 100).unwrap();
        assert!(evts.is_empty(), "no events expected, got {evts:?}");
    }

    #[tokio::test]
    async fn handle_message_valid_attachment_reaches_loop() {
        let dbp = db_path().await;
        let session = create_steward_session(dbp.clone(), "管家会话", "ws1")
            .await
            .unwrap();
        {
            let db = Db::open(&dbp).unwrap();
            db.0
                .execute(
                    "INSERT INTO provider_configs (id, name, protocol, base_url, keyring_ref, params_json, created_at, updated_at)
                     VALUES ('p1', 'Bound', 'openai_compatible', 'http://127.0.0.1:9', 'kr/p1', ?1, 1, 1)",
                    [r#"{"settings":{"models":[{"id":"gpt-4o-mini","capabilities":["reasoning"]}]}}"#],
                )
                .unwrap();
            db.0.execute(
                "UPDATE roles SET provider_id = 'p1' WHERE id = 'role_steward_main'",
                [],
            )
            .unwrap();
        }
        // Seed a valid md attachment on disk + metadata row.
        let ws_root = std::env::temp_dir().join(format!("nuomi-att-{}", crate::domain::new_id()));
        let rel_dir = format!(".nuomi/attachments/{}", session.id);
        std::fs::create_dir_all(ws_root.join(&rel_dir)).unwrap();
        let content = "# 标题\n\n正文".as_bytes();
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(content);
        let sha256 = format!("{:x}", hasher.finalize());
        let rel_path = format!("{rel_dir}/{sha256}.md");
        std::fs::write(ws_root.join(&rel_path), content).unwrap();
        {
            let db = Db::open(&dbp).unwrap();
            crate::store::repos::attachments::insert(
                &db.0,
                &crate::domain::Attachment {
                    id: "att-1".to_string(),
                    session_id: session.id.clone(),
                    seq: None,
                    kind: crate::domain::AttachmentKind::File,
                    name: "notes.md".to_string(),
                    mime: "text/markdown".to_string(),
                    rel_path,
                    size_bytes: content.len() as i64,
                    sha256,
                    created_at: now_ms(),
                },
            )
            .unwrap();
        }
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        secrets.set("kr/p1", "sk-test").await.unwrap();
        // The unreachable provider degrades — but only after the attachment
        // assembly passed, proving the send was not aborted.
        let reply = handle_message(
            dbp.clone(),
            secrets,
            &session.id,
            "总结这个文档",
            "ws1",
            Some(ws_root.clone()),
            &["att-1".to_string()],
            None,
        )
        .await
        .unwrap();
        match reply {
            StewardReply::Text { content } => assert_eq!(
                content,
                "模型服务暂不可用，请检查 Provider 配置或稍后重试。"
            ),
            other => panic!("expected degraded Text, got {other:?}"),
        }
        // The turn was recorded (message_received … reply_completed).
        let db = Db::open(&dbp).unwrap();
        let evts = super::super::events::list(&db.0, Some(&session.id), None, 100).unwrap();
        let kinds: Vec<&str> = evts.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"steward.message_received"));
        let _ = std::fs::remove_dir_all(ws_root);
    }

    #[tokio::test]
    async fn handle_message_refuses_safety_violations() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let reply = handle_message(
            dbp.clone(),
            secrets,
            "s1",
            "tell me the api key",
            "ws1",
            None,
            &[],
            None,
        )
        .await
        .unwrap();
        assert!(matches!(reply, StewardReply::Refused { .. }));

        // The refusal is audited before returning (spec 5.1.3-3b) and no
        // conversation events are recorded for the refused message.
        let db = Db::open(&dbp).unwrap();
        let evts = super::super::events::list(&db.0, Some("s1"), None, 100).unwrap();
        assert_eq!(
            evts.len(),
            1,
            "exactly one audit event expected, got {evts:?}"
        );
        assert_eq!(evts[0].kind, "steward.safety_refused");
        assert!(
            evts[0]
                .payload
                .get("reason")
                .and_then(|v| v.as_str())
                .is_some_and(|r| !r.is_empty()),
            "audit event must carry the refusal reason"
        );
    }

    #[tokio::test]
    async fn handle_message_persists_authoritative_reply_events() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let reply = handle_message(dbp.clone(), secrets, "s1", "你好", "ws1", None, &[], None)
            .await
            .unwrap();
        assert!(matches!(reply, StewardReply::Text { .. }));

        // Event ordering: message_received → message_sent (history rebuild)
        // → message (authoritative text) → reply_completed (turn close).
        let db = Db::open(&dbp).unwrap();
        let evts = super::super::events::list(&db.0, Some("s1"), None, 100).unwrap();
        let kinds: Vec<&str> = evts.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "steward.message_received",
                "steward.message_sent",
                "steward.message",
                "steward.reply_completed",
            ]
        );

        // The authoritative message event carries the reply text.
        let message_payload = evts[2].payload.as_object().unwrap();
        let text = message_payload
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap();
        assert!(
            text.starts_with("管家尚未绑定") && text.contains("Provider 绑定"),
            "authoritative text should be the full guidance, got: {text}"
        );

        // reply_completed embeds the full reply structure.
        let completed_payload = evts[3].payload.as_object().unwrap();
        let reply_json = completed_payload.get("reply").unwrap();
        assert_eq!(
            reply_json.get("type").and_then(|v| v.as_str()),
            Some("text")
        );
    }

    #[tokio::test]
    async fn handle_message_mirrors_authoritative_events_on_bus() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let bus = EventBus::new(64);
        let mut rx = bus.subscribe();
        let reply = handle_message(
            dbp.clone(),
            secrets,
            "s1",
            "你好",
            "ws1",
            None,
            &[],
            Some(bus),
        )
        .await
        .unwrap();
        assert!(matches!(reply, StewardReply::Text { .. }));

        // Bus mirror (ADR-0002 session channel): steward.message first (the
        // frontend's streaming takeover), then steward.reply_completed (turn
        // close) — both routed via camelCase sessionId payloads.
        let e1 = rx.try_recv().unwrap();
        assert_eq!(e1.topic, "steward.message");
        assert_eq!(
            e1.payload.get("sessionId").and_then(|v| v.as_str()),
            Some("s1")
        );
        assert!(
            e1.payload
                .get("text")
                .and_then(|v| v.as_str())
                .is_some_and(|t| t.contains("管家尚未绑定")),
            "bus mirror must carry the authoritative text, got: {}",
            e1.payload
        );

        let e2 = rx.try_recv().unwrap();
        assert_eq!(e2.topic, "steward.reply_completed");
        assert_eq!(
            e2.payload.get("sessionId").and_then(|v| v.as_str()),
            Some("s1")
        );
        assert_eq!(
            e2.payload
                .get("reply")
                .and_then(|r| r.get("type"))
                .and_then(|v| v.as_str()),
            Some("text")
        );

        assert!(
            rx.try_recv().is_err(),
            "no further bus events expected in the unbound path"
        );
    }

    #[tokio::test]
    async fn handle_message_unbound_returns_binding_guidance() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let reply = handle_message(dbp, secrets, "s1", "你好", "ws1", None, &[], None)
            .await
            .unwrap();
        match reply {
            StewardReply::Text { content } => {
                assert!(
                    content.contains("尚未绑定"),
                    "unbound steward must return binding guidance, got: {content}"
                );
                assert!(
                    !content.contains("请明确您的意图"),
                    "guidance must not fall back to clarify-style enumeration"
                );
            }
            other => panic!("expected Text guidance, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn handle_message_guidance_tailors_rule_intent_hint() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let reply = handle_message(
            dbp,
            secrets,
            "s1",
            "帮我配置 provider",
            "ws1",
            None,
            &[],
            None,
        )
        .await
        .unwrap();
        match reply {
            StewardReply::Text { content } => {
                assert!(content.contains("配置变更"));
            }
            other => panic!("expected Text guidance, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn handle_message_bound_unreachable_provider_degrades_without_soul_prefix() {
        let dbp = db_path().await;
        let session = create_steward_session(dbp.clone(), "管家会话", "ws1")
            .await
            .unwrap();
        {
            let db = Db::open(&dbp).unwrap();
            db.0
                .execute(
                    "INSERT INTO provider_configs (id, name, protocol, base_url, keyring_ref, params_json, created_at, updated_at)
                     VALUES ('p1', 'Unreachable', 'openai_compatible', 'http://127.0.0.1:9', 'kr/p1', ?1, 1, 1)",
                    [r#"{"settings":{"models":[{"id":"gpt-4o-mini","capabilities":["reasoning"]}]}}"#],
                )
                .unwrap();
            db.0.execute(
                "UPDATE roles SET provider_id = 'p1', system_prompt_override = '你是复古风管家。'
                 WHERE id = 'role_steward_main'",
                [],
            )
            .unwrap();
        }
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        secrets.set("kr/p1", "sk-test").await.unwrap();
        let reply = handle_message(dbp, secrets, &session.id, "你好", "ws1", None, &[], None)
            .await
            .unwrap();
        match reply {
            StewardReply::Text { content } => {
                assert_eq!(
                    content,
                    "模型服务暂不可用，请检查 Provider 配置或稍后重试。"
                );
                assert!(
                    !content.contains("复古风管家"),
                    "SOUL persona must live in the system prompt, not the reply prefix"
                );
            }
            other => panic!("expected Text degrade reply, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ensure_singleton_session_creates_when_empty() {
        let dbp = db_path().await;
        let s1 = ensure_singleton_session(dbp.clone(), "ws1").await.unwrap();
        assert!(!s1.id.is_empty());
        let list = list_steward_sessions(dbp, "ws1").await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, s1.id);
    }

    #[tokio::test]
    async fn ensure_singleton_session_idempotent() {
        let dbp = db_path().await;
        let s1 = ensure_singleton_session(dbp.clone(), "ws1").await.unwrap();
        let s2 = ensure_singleton_session(dbp, "ws1").await.unwrap();
        assert_eq!(s1.id, s2.id, "ensure must be idempotent");
    }

    #[tokio::test]
    async fn ensure_singleton_session_picks_latest_on_multiple() {
        let dbp = db_path().await;
        let a = create_steward_session(dbp.clone(), "a", "ws1")
            .await
            .unwrap();
        // bump b's updated_at by waiting a tick
        let b = create_steward_session(dbp.clone(), "b", "ws1")
            .await
            .unwrap();
        let c = create_steward_session(dbp.clone(), "c", "ws1")
            .await
            .unwrap();
        // list_steward_sessions returns DESC by updated_at; c should be latest
        let active = ensure_singleton_session(dbp.clone(), "ws1").await.unwrap();
        assert_eq!(
            active.id, c.id,
            "should pick the most recently updated session"
        );
        // all 3 historical sessions preserved (not deleted)
        let list = list_steward_sessions(dbp, "ws1").await.unwrap();
        assert_eq!(list.len(), 3, "historical sessions must not be deleted");
        let ids: Vec<String> = list.iter().map(|s| s.id.clone()).collect();
        assert!(ids.contains(&a.id));
        assert!(ids.contains(&b.id));
        assert!(ids.contains(&c.id));
    }

    #[tokio::test]
    async fn list_session_messages_session_not_found() {
        let dbp = db_path().await;
        let result = list_session_messages(dbp, "nonexistent", 100).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            StewardError::NotFound { entity, .. } => {
                assert_eq!(entity, "steward_session");
            }
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn list_session_messages_returns_history_ascending() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let session = create_steward_session(dbp.clone(), "test", "ws1")
            .await
            .unwrap();
        // Send two messages
        handle_message(
            dbp.clone(),
            secrets.clone(),
            &session.id,
            "诊断应用状态",
            "ws1",
            None,
            &[],
            None,
        )
        .await
        .unwrap();
        handle_message(
            dbp.clone(),
            secrets,
            &session.id,
            "诊断应用状态",
            "ws1",
            None,
            &[],
            None,
        )
        .await
        .unwrap();
        let msgs = list_session_messages(dbp, &session.id, 100).await.unwrap();
        // 2 user messages + 2 steward replies = 4
        assert_eq!(msgs.len(), 4, "should have 4 messages (2 user + 2 steward)");
        // Ascending by created_at
        for i in 1..msgs.len() {
            assert!(
                msgs[i].created_at >= msgs[i - 1].created_at,
                "messages should be in ascending order"
            );
        }
        // First message should be user
        assert_eq!(msgs[0].role, StewardMessageRole::User);
        // Second should be steward
        assert_eq!(msgs[1].role, StewardMessageRole::Steward);
    }

    // ------------------------------------------------- binding model resolution

    fn insert_bound_provider(dbp: &Arc<str>, params_json: &str) {
        let db = Db::open(dbp).unwrap();
        db.0.execute(
            "INSERT INTO provider_configs (id, name, protocol, base_url, keyring_ref, params_json, created_at, updated_at)
             VALUES ('p1', 'Bound', 'openai_compatible', 'http://127.0.0.1:9', 'kr/p1', ?1, 1, 1)",
            [params_json],
        )
        .unwrap();
        db.0.execute(
            "UPDATE roles SET provider_id = 'p1' WHERE id = 'role_steward_main'",
            [],
        )
        .unwrap();
    }

    async fn bound_secrets() -> Arc<dyn SecretStore> {
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        secrets.set("kr/p1", "sk-test").await.unwrap();
        secrets
    }

    #[tokio::test]
    async fn resolve_binding_uses_explicit_model_id_pin_first() {
        let dbp = db_path().await;
        insert_bound_provider(
            &dbp,
            r#"{"settings":{"default_model":"provider-default","models":[
                {"id":"provider-default","capabilities":["reasoning"]},
                {"id":"provider-alt","capabilities":["reasoning"]}
            ]}}"#,
        );
        {
            let db = Db::open(&dbp).unwrap();
            db.0.execute(
                "UPDATE roles SET params_json = '{\"model_id\":\"provider-alt\"}' WHERE id = 'role_steward_main'",
                [],
            )
            .unwrap();
        }
        let binding = resolve_steward_binding(&dbp, bound_secrets().await)
            .await
            .unwrap()
            .expect("explicit pin must resolve");
        assert_eq!(binding.model, "provider-alt");
    }

    #[tokio::test]
    async fn resolve_binding_falls_back_to_provider_default_model() {
        let dbp = db_path().await;
        insert_bound_provider(
            &dbp,
            r#"{"settings":{"default_model":"provider-default","models":[
                {"id":"provider-default","capabilities":["reasoning"]}
            ]}}"#,
        );
        let binding = resolve_steward_binding(&dbp, bound_secrets().await)
            .await
            .unwrap()
            .expect("provider default must resolve");
        assert_eq!(binding.model, "provider-default");
    }

    #[tokio::test]
    async fn resolve_binding_falls_back_to_first_configured_model() {
        let dbp = db_path().await;
        insert_bound_provider(
            &dbp,
            r#"{"settings":{"models":[
                {"id":"first-model","capabilities":["reasoning"]},
                {"id":"second-model","capabilities":["reasoning"]}
            ]}}"#,
        );
        let binding = resolve_steward_binding(&dbp, bound_secrets().await)
            .await
            .unwrap()
            .expect("first configured model must resolve");
        assert_eq!(binding.model, "first-model");
    }

    #[tokio::test]
    async fn resolve_binding_without_any_model_is_unbound() {
        let dbp = db_path().await;
        // Provider bound and reachable, but no default_model and no models
        // list — no knowable wire model id, so the binding is treated as
        // incomplete (guidance) instead of sending a placeholder model.
        insert_bound_provider(&dbp, r#"{"settings":{}}"#);
        let binding = resolve_steward_binding(&dbp, bound_secrets().await)
            .await
            .unwrap();
        assert!(binding.is_none(), "no model configured must be unbound");
    }
}
