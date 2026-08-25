//! Kernel assembly facade: one struct that boots a fully wired harness
//! kernel (storage + provider + plugins) for the headless CLI and tests.
//!
//! SPEC T10 / AC17–AC18. The CLI reads `NUOMI_API_KEY` from the environment;
//! keys are never persisted or logged.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;
use tokio::sync::Mutex;

use crate::domain::{new_id, now_ms, EventRecord, ProviderProtocol, Session};
use crate::harness::Context;
use crate::plugins::{
    DeltaCallback, HookRegistry, LoopConfig, LoopEngine, LoopRunResult, MemoryService,
    SystemPromptService, ToolRegistry,
};
use crate::providers::{
    AnthropicCompatibleClient, ChatMessage, ChatResponse, FakeLlm, LlmProvider, MessageRole,
    OpenAiCompatibleClient,
};
use crate::store::{migrations, repos, Db};
use crate::{CoreError, CoreResult};

const DEFAULT_SYSTEM_PROMPT: &str = "You are nuomi, a helpful agent.";

/// A concrete model endpoint (CLI v1: key comes from `NUOMI_API_KEY`).
#[derive(Debug, Clone)]
pub struct ProviderEndpoint {
    pub protocol: ProviderProtocol,
    pub base_url: String,
    /// Passed in memory only; never written to disk or logs.
    pub api_key: String,
    pub model: String,
}

/// Which LLM backs the kernel.
#[derive(Debug, Clone)]
pub enum ProviderSource {
    Endpoint(ProviderEndpoint),
    /// Deterministic playback provider — shared by tests and demo mode.
    Fake(Vec<ChatResponse>),
}

#[derive(Debug, Clone)]
pub struct NuomiConfig {
    pub db_path: PathBuf,
    pub provider: ProviderSource,
}

impl NuomiConfig {
    pub fn new(db_path: PathBuf, endpoint: ProviderEndpoint) -> Self {
        Self {
            db_path,
            provider: ProviderSource::Endpoint(endpoint),
        }
    }

    /// TEST/DEMO-ONLY: boots against a scripted [`FakeLlm`] instead of a
    /// real endpoint.
    pub fn with_fake_provider(db_path: PathBuf, script: Vec<ChatResponse>) -> Self {
        Self {
            db_path,
            provider: ProviderSource::Fake(script),
        }
    }
}

struct SessionState {
    session_id: String,
    /// Rebuilt transcript prefix (populated by `resume`, extended after
    /// every successful run).
    history: Vec<ChatMessage>,
}

/// A booted kernel bound to one database and one provider.
pub struct NuomiKernel {
    db_path: Arc<str>,
    model: String,
    provider: Arc<dyn LlmProvider>,
    ctx: Context,
    tools: Arc<ToolRegistry>,
    hooks: Arc<HookRegistry>,
    memory: MemoryService,
    delta_cb: Option<DeltaCallback>,
    state: Mutex<SessionState>,
}

impl NuomiKernel {
    /// Opens the database, runs migrations, creates the first session row,
    /// builds the provider and registers the core plugins.
    pub async fn boot(config: NuomiConfig) -> CoreResult<Self> {
        let db_path: Arc<str> = Arc::from(config.db_path.to_string_lossy().to_string());
        let session_id = {
            let path = db_path.clone();
            tokio::task::spawn_blocking(move || -> Result<String, CoreError> {
                let db = Db::open(&path)?;
                migrations::run(&db.0)?;
                let session = Session {
                    id: new_id(),
                    title: "nuomi session".into(),
                    created_at: now_ms(),
                    updated_at: now_ms(),
                };
                repos::sessions::insert(&db.0, &session)?;
                Ok(session.id)
            })
            .await
            .map_err(join_err)??
        };

        let model = match &config.provider {
            ProviderSource::Endpoint(ep) => ep.model.clone(),
            ProviderSource::Fake(_) => "fake-model".to_string(),
        };
        let provider: Arc<dyn LlmProvider> = match config.provider {
            ProviderSource::Endpoint(ep) => match ep.protocol {
                ProviderProtocol::OpenAiCompatible => {
                    Arc::new(OpenAiCompatibleClient::new(ep.base_url, ep.api_key))
                }
                ProviderProtocol::AnthropicCompatible => {
                    Arc::new(AnthropicCompatibleClient::new(ep.base_url, ep.api_key))
                }
            },
            ProviderSource::Fake(script) => Arc::new(FakeLlm::new("fake", script)),
        };

        let ctx = Context::default();
        let system_prompt = SystemPromptService::with_store(db_path.clone(), DEFAULT_SYSTEM_PROMPT);
        ctx.register_service("system_prompt", "", Arc::new(system_prompt))
            .await?;
        let memory = MemoryService::new(db_path.clone());
        ctx.register_service("memory", "", Arc::new(memory.clone()))
            .await?;

        Ok(Self {
            db_path,
            model,
            provider,
            ctx,
            tools: Arc::new(ToolRegistry::new()),
            hooks: Arc::new(HookRegistry::new()),
            memory,
            delta_cb: None,
            state: Mutex::new(SessionState {
                session_id,
                history: Vec::new(),
            }),
        })
    }

    /// Installs a streaming delta observer (CLI stdout echo).
    pub fn with_delta_callback(mut self, cb: Option<DeltaCallback>) -> Self {
        self.delta_cb = cb;
        self
    }

    /// Kernel event-bus access (used by the Tauri shell event bridge).
    pub fn context(&self) -> &Context {
        &self.ctx
    }

    pub async fn session_id(&self) -> String {
        self.state.lock().await.session_id.clone()
    }

    /// Runs one user task through the Loop Engine, persists the transcript
    /// as append-only session events, and extends the in-memory history.
    pub async fn run_task(&self, task: &str) -> CoreResult<LoopRunResult> {
        let mut state = self.state.lock().await;
        let session_id_for_delta = state.session_id.clone();
        // Bridge streaming deltas onto the kernel bus so the shell's event
        // bridge can forward them on `event://session/{id}` (ADR-0002).
        let user_cb = self.delta_cb.clone();
        let ctx = self.ctx.clone();
        let delta_bridge: Option<DeltaCallback> = Some(Arc::new(move |delta: String| {
            if let Some(cb) = &user_cb {
                cb(delta.clone());
            }
            ctx.publish(crate::harness::Event::new(
                "session.delta",
                serde_json::json!({ "sessionId": session_id_for_delta, "text": delta }),
            ));
        }));
        let engine = LoopEngine::new(
            self.provider.clone(),
            LoopConfig {
                model: self.model.clone(),
                ..LoopConfig::default()
            },
        )
        .with_delta_callback(delta_bridge);

        let history = std::mem::take(&mut state.history);
        let history_len = history.len();
        let result = engine
            .run_with_history(
                &self.ctx,
                &self.tools,
                Some(&self.hooks),
                Some(&self.memory),
                history,
                task,
            )
            .await?;

        // Persist only the new messages — replayed history is already in
        // the append-only log.
        self.persist_transcript(&state.session_id, &result.transcript[history_len..])
            .await?;
        state.history = result.transcript.clone();
        Ok(result)
    }

    /// Switches to an existing session: replays its event log into a
    /// `ChatMessage` sequence that seeds the next run (AC17 resume).
    pub async fn resume(&self, session_id: &str) -> CoreResult<()> {
        let path = self.db_path.clone();
        let sid = session_id.to_string();
        let history =
            tokio::task::spawn_blocking(move || -> Result<Vec<ChatMessage>, CoreError> {
                let db = Db::open(&path)?;
                repos::sessions::get(&db.0, &sid)?;
                let events = repos::events::list_by_aggregate(&db.0, "session", &sid, None)?;
                Ok(rebuild_history(&events))
            })
            .await
            .map_err(join_err)??;

        let mut state = self.state.lock().await;
        state.session_id = session_id.to_string();
        state.history = history;
        Ok(())
    }

    /// Starts a fresh session (`/new` in the REPL).
    pub async fn new_session(&self) -> CoreResult<String> {
        let path = self.db_path.clone();
        let session = Session {
            id: new_id(),
            title: "nuomi session".into(),
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let id = session.id.clone();
        tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
            let db = Db::open(&path)?;
            migrations::run(&db.0)?;
            repos::sessions::insert(&db.0, &session)?;
            Ok(())
        })
        .await
        .map_err(join_err)??;

        let mut state = self.state.lock().await;
        state.session_id = id.clone();
        state.history = Vec::new();
        Ok(id)
    }

    pub async fn list_sessions(&self) -> CoreResult<Vec<Session>> {
        let path = self.db_path.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Session>, CoreError> {
            let db = Db::open(&path)?;
            migrations::run(&db.0)?;
            Ok(repos::sessions::list(&db.0, 100)?)
        })
        .await
        .map_err(join_err)?
    }

    async fn persist_transcript(
        &self,
        session_id: &str,
        transcript: &[ChatMessage],
    ) -> CoreResult<()> {
        let path = self.db_path.clone();
        let sid = session_id.to_string();
        let messages = transcript.to_vec();
        tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
            let db = Db::open(&path)?;
            let conn = &db.0;
            let now = now_ms();
            for m in &messages {
                match m.role {
                    MessageRole::User => {
                        repos::events::append(
                            conn,
                            "session",
                            &sid,
                            "message",
                            &json!({ "role": "user", "content": m.content }),
                            now,
                        )?;
                    }
                    MessageRole::Assistant => {
                        for call in &m.tool_calls {
                            repos::events::append(
                                conn,
                                "session",
                                &sid,
                                "tool_call",
                                &json!({ "tool": call.name, "arguments": call.arguments }),
                                now,
                            )?;
                        }
                        repos::events::append(
                            conn,
                            "session",
                            &sid,
                            "message",
                            &json!({ "role": "assistant", "content": m.content }),
                            now,
                        )?;
                    }
                    MessageRole::Tool => {
                        repos::events::append(
                            conn,
                            "session",
                            &sid,
                            "tool_result",
                            &json!({
                                "call_id": m.tool_call_id.clone().unwrap_or_default(),
                                "content": m.content,
                            }),
                            now,
                        )?;
                    }
                    MessageRole::System => {}
                }
            }
            repos::sessions::touch(conn, &sid, now)?;
            Ok(())
        })
        .await
        .map_err(join_err)??;
        Ok(())
    }
}

/// Rebuilds a `ChatMessage` sequence from persisted session events
/// (message/tool_result kinds; tool_call details are folded into the
/// assistant message on the wire, so replay keeps ordering only).
fn rebuild_history(events: &[EventRecord]) -> Vec<ChatMessage> {
    events
        .iter()
        .filter_map(|e| match e.kind.as_str() {
            "message" => match e.payload.get("role").and_then(|r| r.as_str()) {
                Some("user") => e
                    .payload
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(ChatMessage::user),
                Some("assistant") => e
                    .payload
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(ChatMessage::assistant),
                _ => None,
            },
            "tool_result" => {
                let call_id = e
                    .payload
                    .get("call_id")
                    .and_then(|c| c.as_str())
                    .unwrap_or_default();
                e.payload
                    .get("content")
                    .and_then(|c| c.as_str())
                    .map(|content| ChatMessage::tool_result(call_id, content))
            }
            _ => None,
        })
        .collect()
}

fn join_err(e: tokio::task::JoinError) -> CoreError {
    CoreError::Store(crate::store::StoreError::Sqlite(
        rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_config(path: PathBuf) -> NuomiConfig {
        NuomiConfig::with_fake_provider(
            path,
            vec![
                FakeLlm::response("first answer"),
                FakeLlm::response("second answer"),
            ],
        )
    }

    #[tokio::test]
    async fn boot_creates_session_and_run_persists_events() {
        let dir = tempfile::tempdir().unwrap();
        let kernel = NuomiKernel::boot(fake_config(dir.path().join("k.db")))
            .await
            .unwrap();

        let result = kernel.run_task("hello there").await.unwrap();
        assert_eq!(result.final_text, "first answer");
        assert_eq!(result.steps, 1);

        // Events were appended under aggregate_type="session".
        let path = dir.path().join("k.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        let events =
            repos::events::list_by_aggregate(&conn, "session", &kernel.session_id().await, None)
                .unwrap();
        let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"message"));
        let user_msg = events
            .iter()
            .find(|e| e.kind == "message" && e.payload["role"] == "user")
            .unwrap();
        assert_eq!(user_msg.payload["content"], "hello there");
    }

    #[tokio::test]
    async fn resume_replays_history_and_continues_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("k.db");

        // First kernel instance: run one turn then "shut down".
        let first = NuomiKernel::boot(fake_config(db_path.clone()))
            .await
            .unwrap();
        let session_id = first.session_id().await;
        first.run_task("hello").await.unwrap();

        // Second instance over the same db: resume + continue (own script).
        let second = NuomiKernel::boot(NuomiConfig::with_fake_provider(
            db_path.clone(),
            vec![FakeLlm::response("second answer")],
        ))
        .await
        .unwrap();
        second.resume(&session_id).await.unwrap();
        let result = second.run_task("more").await.unwrap();
        assert_eq!(result.final_text, "second answer");
        assert_eq!(result.transcript.len(), 4); // u/a from turn 1 + new user + reply

        // Both turns are persisted in order.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let events = repos::events::list_by_aggregate(&conn, "session", &session_id, None).unwrap();
        let user_contents: Vec<&str> = events
            .iter()
            .filter(|e| e.kind == "message" && e.payload["role"] == "user")
            .filter_map(|e| e.payload["content"].as_str())
            .collect();
        assert_eq!(user_contents, vec!["hello", "more"]);
    }

    #[tokio::test]
    async fn resume_unknown_session_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let kernel = NuomiKernel::boot(fake_config(dir.path().join("k.db")))
            .await
            .unwrap();
        assert!(kernel.resume("no-such-session").await.is_err());
    }

    #[tokio::test]
    async fn list_sessions_and_new_session_work() {
        let dir = tempfile::tempdir().unwrap();
        let kernel = NuomiKernel::boot(fake_config(dir.path().join("k.db")))
            .await
            .unwrap();
        let sessions = kernel.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);

        let new_id = kernel.new_session().await.unwrap();
        let sessions = kernel.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.iter().any(|s| s.id == new_id));
        // History was reset with the fresh session.
        let result = kernel.run_task("fresh start").await.unwrap();
        assert_eq!(result.transcript.len(), 2);
    }
}
