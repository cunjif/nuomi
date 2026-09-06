//! Kernel assembly facade: one struct that boots a fully wired harness
//! kernel (storage + provider + plugins) for the headless CLI and tests.
//!
//! SPEC T10 / AC17–AC18. The CLI reads `NUOMI_API_KEY` from the environment;
//! keys are never persisted or logged.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
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

/// Title for brand-new sessions until the first user task derives a real
/// one (shell and CLI share this via the sessions repo).
const DEFAULT_SESSION_TITLE: &str = "nuomi session";

/// Auto-derived titles never exceed this many characters.
const TITLE_MAX_CHARS: usize = 40;

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
    /// Live-stream delta ordinal for the active session (1-based, in-memory
    /// only, reset whenever the session switches). NON-PERSISTENT by design:
    /// it exists purely so consumers can dedupe replays and restore order
    /// within one live `session.delta` stream. The authoritative ordering seq
    /// for durable rows stays the per-aggregate `seq` assigned by the
    /// `events` table (`repos::events::append`) — the two never mix.
    delta_seq: Arc<AtomicU64>,
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
        let boot_started = std::time::Instant::now();
        let db_path: Arc<str> = Arc::from(config.db_path.to_string_lossy().to_string());
        let session_id = {
            let path = db_path.clone();
            tokio::task::spawn_blocking(move || -> Result<String, CoreError> {
                let db = Db::open(&path)?;
                migrations::run(&db.0)?;
                let session = Session {
                    id: new_id(),
                    title: DEFAULT_SESSION_TITLE.into(),
                    created_at: now_ms(),
                    updated_at: now_ms(),
                };
                repos::sessions::insert(&db.0, &session)?;
                Ok(session.id)
            })
            .await
            .map_err(join_err)??
        };
        tracing::info!(
            db_boot_ms = boot_started.elapsed().as_millis() as u64,
            "kernel database boot complete (open + migrations + first session)"
        );

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
                delta_seq: Arc::new(AtomicU64::new(0)),
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
        let delta_counter = state.delta_seq.clone();
        // Bridge streaming deltas onto the kernel bus so the shell's event
        // bridge can forward them on `event://session/{id}` (ADR-0002).
        //
        // The injected `seq` is a NON-PERSISTENT live-stream ordinal (1-based,
        // in-memory counter reset per session). It lets the frontend dedupe
        // replayed deltas and restore ordering within one live stream; it has
        // no relationship to the durable `seq` of rows in the `events` table.
        let user_cb = self.delta_cb.clone();
        let ctx = self.ctx.clone();
        let delta_bridge: Option<DeltaCallback> = Some(Arc::new(move |delta: String| {
            let seq = delta_counter.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(cb) = &user_cb {
                cb(delta.clone());
            }
            ctx.publish(crate::harness::Event::new(
                "session.delta",
                serde_json::json!({ "sessionId": session_id_for_delta, "text": delta, "seq": seq }),
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
        // the append-only log. The auto-title rides the same persistence
        // phase: one spawn_blocking round-trip, check-then-update so the
        // derived name is written exactly once and never overwritten.
        let auto_title = derive_title(task);
        let appended = self
            .persist_transcript(
                &state.session_id,
                &result.transcript[history_len..],
                &auto_title,
            )
            .await?;
        state.history = result.transcript.clone();

        // Publish what was just persisted (iron rule: persist first, then
        // emit). Each record rides the session channel carrying its
        // AUTHORITATIVE `seq` — the per-aggregate `events`-table seq used for
        // gap recovery via `listEvents(afterSeq)` — which is a different,
        // unrelated number from the live delta ordinal above. Assistant
        // messages additionally carry `deltaTo`: the live-delta high-water
        // mark this answer covers, so the UI can swap buffer→full text and
        // retire exactly that delta range in one step.
        let total_deltas = state.delta_seq.load(Ordering::Relaxed);
        for rec in &appended {
            let mut body = rec.payload.clone();
            if let Some(obj) = body.as_object_mut() {
                obj.insert("sessionId".into(), json!(state.session_id));
                obj.insert("seq".into(), json!(rec.seq));
                let is_assistant_message = rec.kind == "message"
                    && rec.payload.get("role").and_then(|r| r.as_str()) == Some("assistant");
                if is_assistant_message {
                    obj.insert("deltaTo".into(), json!(total_deltas));
                }
            }
            self.ctx
                .publish(crate::harness::Event::new("session.message", body));
        }
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
        // Different live stream ⇒ restart the non-persistent delta ordinal.
        state.delta_seq = Arc::new(AtomicU64::new(0));
        Ok(())
    }

    /// Starts a fresh session (`/new` in the REPL).
    pub async fn new_session(&self) -> CoreResult<String> {
        let path = self.db_path.clone();
        let session = Session {
            id: new_id(),
            title: DEFAULT_SESSION_TITLE.into(),
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
        // Fresh session ⇒ the live delta ordinal restarts at 1.
        state.delta_seq = Arc::new(AtomicU64::new(0));
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
        auto_title: &str,
    ) -> CoreResult<Vec<EventRecord>> {
        let path = self.db_path.clone();
        let sid = session_id.to_string();
        let messages = transcript.to_vec();
        let auto_title = auto_title.to_string();
        tokio::task::spawn_blocking(move || -> Result<Vec<EventRecord>, CoreError> {
            let db = Db::open(&path)?;
            let conn = &db.0;
            let now = now_ms();
            let mut appended = Vec::new();
            for m in &messages {
                match m.role {
                    MessageRole::User => {
                        appended.push(repos::events::append(
                            conn,
                            "session",
                            &sid,
                            "message",
                            &json!({ "role": "user", "content": m.content }),
                            now,
                        )?);
                    }
                    MessageRole::Assistant => {
                        for call in &m.tool_calls {
                            appended.push(repos::events::append(
                                conn,
                                "session",
                                &sid,
                                "tool_call",
                                &json!({ "tool": call.name, "arguments": call.arguments }),
                                now,
                            )?);
                        }
                        appended.push(repos::events::append(
                            conn,
                            "session",
                            &sid,
                            "message",
                            &json!({ "role": "assistant", "content": m.content }),
                            now,
                        )?);
                    }
                    MessageRole::Tool => {
                        appended.push(repos::events::append(
                            conn,
                            "session",
                            &sid,
                            "tool_result",
                            &json!({
                                "call_id": m.tool_call_id.clone().unwrap_or_default(),
                                "content": m.content,
                            }),
                            now,
                        )?);
                    }
                    MessageRole::System => {}
                }
            }
            // Name a still-default session once from its first task
            // (check-then-update: later tasks must not overwrite).
            if !auto_title.is_empty()
                && repos::sessions::get(conn, &sid)?.title == DEFAULT_SESSION_TITLE
            {
                repos::sessions::update_title(conn, &sid, &auto_title)?;
            }
            repos::sessions::touch(conn, &sid, now)?;
            Ok(appended)
        })
        .await
        .map_err(join_err)?
    }
}

/// Derives a session auto-title from the first task input: the trimmed
/// first line, capped at [`TITLE_MAX_CHARS`] characters with a trailing
/// ellipsis only when truncation happened. Whitespace-only input maps to
/// `""` (meaning "keep the default title").
fn derive_title(input: &str) -> String {
    let first_line = input.trim().lines().next().unwrap_or("").trim_end();
    let mut title: String = first_line.chars().take(TITLE_MAX_CHARS).collect();
    if first_line.chars().count() > TITLE_MAX_CHARS {
        title.push('…');
    }
    title
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

    #[test]
    fn derive_title_table() {
        let exact = "x".repeat(TITLE_MAX_CHARS);
        let overlong_ascii = "x".repeat(TITLE_MAX_CHARS + 5);
        let truncated_ascii = format!("{}…", "x".repeat(TITLE_MAX_CHARS));
        let overlong_cjk = "糯".repeat(TITLE_MAX_CHARS + 1);
        let truncated_cjk = format!("{}…", "糯".repeat(TITLE_MAX_CHARS));
        let cases: Vec<(&str, &str)> = vec![
            // Empty / whitespace-only → keep default (empty marker).
            ("", ""),
            ("   \n\t ", ""),
            // Single line, short.
            ("hello", "hello"),
            ("  padded  ", "padded"),
            // Multi-line → first line only, trailing spaces dropped.
            ("first line\nsecond line", "first line"),
            ("trailing spaces   \nnext", "trailing spaces"),
            ("第一行\n第二行", "第一行"),
            // Exactly 40 chars → no ellipsis.
            (exact.as_str(), exact.as_str()),
            // Overlong single line → truncated with ellipsis.
            (overlong_ascii.as_str(), truncated_ascii.as_str()),
            // Multi-byte chars count per character, not per byte.
            (overlong_cjk.as_str(), truncated_cjk.as_str()),
        ];
        for (input, expected) in cases {
            assert_eq!(derive_title(input), expected, "input: {input:?}");
        }
    }

    #[tokio::test]
    async fn first_task_titles_default_session_and_later_tasks_keep_it() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("k.db");
        let kernel = NuomiKernel::boot(NuomiConfig::with_fake_provider(
            db_path.clone(),
            vec![FakeLlm::response("a"), FakeLlm::response("b")],
        ))
        .await
        .unwrap();
        let sid = kernel.session_id().await;

        kernel
            .run_task("fix the login bug\nrepro steps inside")
            .await
            .unwrap();
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            assert_eq!(
                repos::sessions::get(&conn, &sid).unwrap().title,
                "fix the login bug"
            );
        }

        // A second task in the same session must NOT overwrite the title.
        kernel.run_task("now a different task").await.unwrap();
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        assert_eq!(
            repos::sessions::get(&conn, &sid).unwrap().title,
            "fix the login bug"
        );
    }

    /// Drains `session.delta` payload seqs observed on the kernel bus.
    async fn delta_seqs(
        rx: &mut tokio::sync::broadcast::Receiver<crate::harness::Event>,
    ) -> Vec<u64> {
        let mut seqs = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            if ev.topic == "session.delta" {
                seqs.push(
                    ev.payload
                        .get("seq")
                        .and_then(|s| s.as_u64())
                        .unwrap_or_else(|| panic!("delta without seq: {:?}", ev.payload)),
                );
            }
        }
        seqs
    }

    #[tokio::test]
    async fn live_delta_seq_is_strictly_increasing_within_a_session() {
        let dir = tempfile::tempdir().unwrap();
        let kernel = NuomiKernel::boot(fake_config(dir.path().join("k.db")))
            .await
            .unwrap();
        let mut rx = kernel.context().subscribe();

        // Two runs in the SAME session: the counter continues across runs.
        kernel.run_task("one").await.unwrap();
        kernel.run_task("two").await.unwrap();
        assert_eq!(delta_seqs(&mut rx).await, vec![1, 2]);
    }

    #[tokio::test]
    async fn new_session_resets_live_delta_counter() {
        let dir = tempfile::tempdir().unwrap();
        let kernel = NuomiKernel::boot(NuomiConfig::with_fake_provider(
            dir.path().join("k.db"),
            vec![
                FakeLlm::response("first answer"),
                FakeLlm::response("second answer"),
            ],
        ))
        .await
        .unwrap();
        let mut rx = kernel.context().subscribe();

        kernel.run_task("one").await.unwrap();
        assert_eq!(delta_seqs(&mut rx).await, vec![1]);

        kernel.new_session().await.unwrap();
        kernel.run_task("two").await.unwrap();
        // Counter restarted with the fresh session (non-persistent ordinal).
        assert_eq!(delta_seqs(&mut rx).await, vec![1]);
    }

    #[tokio::test]
    async fn persisted_message_events_carry_authoritative_seq_distinct_from_delta_ordinal() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("k.db");
        let kernel = NuomiKernel::boot(fake_config(db_path.clone()))
            .await
            .unwrap();
        let sid = kernel.session_id().await;
        let mut rx = kernel.context().subscribe();

        kernel.run_task("hello").await.unwrap();

        // The fake provider streams exactly one TextDelta per response, so the
        // live ordinal is 1 while the assistant row's persisted seq is 2 —
        // proving the two numbering spaces are independent by construction.
        let mut messages = Vec::new();
        while let Ok(ev) = rx.try_recv() {
            if ev.topic == "session.message" {
                messages.push(ev.payload);
            }
        }
        let user = messages
            .iter()
            .find(|p| p["role"] == "user")
            .expect("user message published");
        let assistant = messages
            .iter()
            .find(|p| p["role"] == "assistant")
            .expect("assistant message published");
        assert_eq!(user["sessionId"], json!(sid));
        assert_eq!(user["seq"], 1); // authoritative events-table seq
        assert_eq!(user.get("deltaTo"), None); // only assistant answers cover deltas

        assert_eq!(assistant["seq"], 2);
        assert_eq!(
            assistant["deltaTo"], 1,
            "deltaTo is the live-delta high-water mark, not the persisted seq"
        );

        // Cross-check the published seqs against the durable rows.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let rows = repos::events::list_by_aggregate(&conn, "session", &sid, None).unwrap();
        assert_eq!(rows[0].seq, user["seq"].as_i64().unwrap());
        assert_eq!(rows[1].seq, assistant["seq"].as_i64().unwrap());
        assert_ne!(
            rows[1].seq as u64, 1,
            "persisted seq must not be confused with the live delta ordinal"
        );
    }
}
