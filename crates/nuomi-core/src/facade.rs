//! Kernel assembly facade: one struct that boots a fully wired harness
//! kernel (storage + provider + plugins) for the headless CLI and tests.
//!
//! SPEC T10 / AC17–AC18. The CLI reads `NUOMI_API_KEY` from the environment;
//! keys are never persisted or logged.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use serde_json::json;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::domain::{new_id, now_ms, EventRecord, ProviderProtocol, Session};
use crate::harness::sideload::{
    apply_to_report, scan, BootReport, LoadOutcome, SideloadedPlugin, Tolerant,
};
use crate::harness::{Context, HarnessError, Kernel};
use crate::plugins::{
    DeltaCallback, HookRegistry, HooksPlugin, LoopConfig, LoopEngine, LoopRunResult, MemoryPlugin,
    MemoryService, SystemPromptPlugin, SystemPromptService, ToolRegistry, ToolsPlugin, TurnHook,
    TurnOverride,
};
use crate::providers::{
    AnthropicCompatibleClient, ChatMessage, ChatResponse, FakeLlm, LlmProvider, MessageRole,
    OpenAiCompatibleClient,
};
use crate::store::{migrations, repos, Db};
use crate::{CoreError, CoreResult};

use crate::services::RoleOverlay;

const DEFAULT_SYSTEM_PROMPT: &str = "You are nuomi, a helpful agent.";

/// Title for brand-new sessions until the first user task derives a real
/// one (shell and CLI share this via the sessions repo).
const DEFAULT_SESSION_TITLE: &str = "nuomi session";

/// Auto-derived titles never exceed this many characters.
const TITLE_MAX_CHARS: usize = 40;

/// Above this character count the input is considered "long" and a
/// keyword-frequency extractor replaces first-line/first-sentence heuristics.
const TITLE_LONG_THRESHOLD: usize = 200;

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
    /// Additional plugin side-load directories (ADR 0009), searched after
    /// `NUOMI_PLUGIN_PATH` and before the user config dir. Empty = defaults.
    pub plugin_paths: Vec<PathBuf>,
}

impl NuomiConfig {
    pub fn new(db_path: PathBuf, endpoint: ProviderEndpoint) -> Self {
        Self {
            db_path,
            provider: ProviderSource::Endpoint(endpoint),
            plugin_paths: Vec::new(),
        }
    }

    /// TEST/DEMO-ONLY: boots against a scripted [`FakeLlm`] instead of a
    /// real endpoint.
    pub fn with_fake_provider(db_path: PathBuf, script: Vec<ChatResponse>) -> Self {
        Self {
            db_path,
            provider: ProviderSource::Fake(script),
            plugin_paths: Vec::new(),
        }
    }

    /// Sets additional plugin side-load directories (ADR 0009).
    pub fn with_plugin_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.plugin_paths = paths;
        self
    }
}

struct SessionState {
    /// `None` until the first operation that needs a session (lazy create
    /// on first `run_task`/`ensure_session_id`). Avoids creating an empty
    /// session row on every boot.
    session_id: Option<String>,
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
    /// The live Kernel holding every registered plugin (built-ins + side-
    /// loaded). Kept alive for the whole NuomiKernel lifetime: dropping it
    /// would tear down side-loaded plugin processes. `shutdown()` runs the
    /// reverse-order dispose (graceful NPP shutdown included).
    kernel: Mutex<Kernel>,
    tools: Arc<ToolRegistry>,
    hooks: Arc<HookRegistry>,
    memory: MemoryService,
    /// Side-load outcomes (loaded/skipped/failed) from boot (ADR 0009).
    boot_report: BootReport,
    delta_cb: Option<DeltaCallback>,
    /// Live-delta ordinals keyed by session id. Kept outside `SessionState`
    /// so the ordinal stays monotonic per session across resumes and across
    /// the isolated `run_task_in_session` path (a per-call counter restarted
    /// at 1 on every message and broke the frontend's delta dedupe).
    delta_seqs: Mutex<HashMap<String, Arc<AtomicU64>>>,
    state: Mutex<SessionState>,
}

impl NuomiKernel {
    /// Opens the database, runs migrations, builds the provider and
    /// registers the core plugins. No session row is created here — the
    /// active session is created lazily on the first `run_task`/
    /// `ensure_session_id` call, so booting no longer leaves an empty
    /// session behind on every launch.
    pub async fn boot(config: NuomiConfig) -> CoreResult<Self> {
        let boot_started = std::time::Instant::now();
        let db_path: Arc<str> = Arc::from(config.db_path.to_string_lossy().to_string());
        {
            let path = db_path.clone();
            tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
                let db = Db::open(&path)?;
                migrations::run(&db.0)?;
                let _ = crate::services::workspace_migration::run_if_needed(&db.0)?;
                Ok(())
            })
            .await
            .map_err(join_err)??;
        }
        tracing::info!(
            db_boot_ms = boot_started.elapsed().as_millis() as u64,
            "kernel database boot complete (open + migrations)"
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

        // The Kernel is the production boot path (ADR 0009 §6): built-ins and
        // side-loaded plugins share one init→start→dispose lifecycle. The
        // facade keeps driving the run loop itself (it needs the run result
        // and delta callbacks synchronously), but every service is born and
        // resolved through the kernel's Context.
        let mut kernel = Kernel::new(Context::default());
        let ctx = kernel.context().clone();
        // Editor bridge: the shell's plugin_editor_call IPC resolves this
        // service to reach side-loaded plugins' editor RPC methods.
        ctx.register_service(
            "kernel",
            "editor_bridge",
            Arc::new(crate::harness::EditorBridgeRegistry::new()),
        )
        .await?;
        kernel.register(Arc::new(ToolsPlugin::default()))?;
        kernel.register(Arc::new(HooksPlugin::new(Arc::new(HookRegistry::new()))))?;
        kernel.register(Arc::new(SystemPromptPlugin::new(
            SystemPromptService::with_store(db_path.clone(), DEFAULT_SYSTEM_PROMPT),
        )))?;
        kernel.register(Arc::new(MemoryPlugin::new(db_path.clone())))?;

        // Side-loaded third-party plugins (ADR 0009). Failures are recorded
        // into the boot report and never abort boot (Tolerant adapter).
        let mut boot_report = BootReport::default();
        let outcomes = scan(&config.plugin_paths);
        apply_to_report(&outcomes, &mut boot_report);
        let report_handle = Arc::new(std::sync::Mutex::new(BootReport::default()));
        for outcome in outcomes {
            if let LoadOutcome::Loaded { dir, manifest, .. } = outcome {
                let plugin = Arc::new(SideloadedPlugin::new(*manifest, dir.clone()));
                let tolerant = Arc::new(Tolerant::new(
                    plugin,
                    dir.display().to_string(),
                    Arc::clone(&report_handle),
                ));
                if let Err(e) = kernel.register(tolerant) {
                    boot_report.record_failed(dir.display().to_string(), e.to_string());
                }
            }
        }

        kernel.boot().await?;
        // Merge what Tolerant recorded during boot (init/start outcomes).
        if let Ok(booted) = report_handle.try_lock() {
            boot_report.loaded.extend(booted.loaded.iter().cloned());
            boot_report.skipped.extend(booted.skipped.iter().cloned());
            boot_report.failed.extend(booted.failed.iter().cloned());
        }
        boot_report.log_summary();

        // The registries the run loop needs are now Context-owned services
        // (qualifier == service name — the shared convention, ADR 0009 §6).
        let missing = |name: &str| {
            CoreError::from(HarnessError::ServiceNotFound {
                name: name.to_string(),
            })
        };
        let tools = ctx
            .service::<ToolRegistry>("tools")
            .await
            .ok_or_else(|| missing("tools"))?;
        let hooks = ctx
            .service::<HookRegistry>("hooks")
            .await
            .ok_or_else(|| missing("hooks"))?;
        let memory = (*ctx
            .service::<MemoryService>("memory")
            .await
            .ok_or_else(|| missing("memory"))?)
        .clone();

        Ok(Self {
            db_path,
            model,
            provider,
            ctx,
            kernel: Mutex::new(kernel),
            tools,
            hooks,
            memory,
            boot_report,
            delta_cb: None,
            delta_seqs: Mutex::new(HashMap::new()),
            state: Mutex::new(SessionState {
                session_id: None,
                history: Vec::new(),
                delta_seq: Arc::new(AtomicU64::new(0)),
            }),
        })
    }

    /// Side-load outcomes (loaded/skipped/failed) from boot (ADR 0009).
    pub fn boot_report(&self) -> &BootReport {
        &self.boot_report
    }

    /// Graceful teardown: disposes started plugins in reverse order (side-
    /// loaded plugins get the NPP `shutdown` handshake before their process
    /// is killed). Errors are collected, never panic.
    pub async fn shutdown(&self) -> Vec<HarnessError> {
        self.kernel.lock().await.shutdown().await
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

    /// Returns the active session id, or `None` if no session has been
    /// created/selected yet (lazy model — boot no longer pre-creates one).
    pub async fn session_id(&self) -> Option<String> {
        self.state.lock().await.session_id.clone()
    }

    /// Returns the active session id, creating one lazily if none exists
    /// yet. Use this when a caller needs a concrete session row (task
    /// dispatch, team runs) rather than the passive [`session_id`].
    pub async fn ensure_session_id(&self) -> CoreResult<String> {
        let mut state = self.state.lock().await;
        if let Some(id) = state.session_id.as_ref() {
            return Ok(id.clone());
        }
        // Hold the state lock across creation so two concurrent callers
        // can't both observe `None` and insert duplicate session rows.
        let id = self.create_session_row().await?;
        state.session_id = Some(id.clone());
        state.history = Vec::new();
        state.delta_seq = self.delta_counter(&id).await;
        Ok(id)
    }

    /// Inserts a fresh session row (shared by `ensure_session_id` and
    /// `new_session`) bound to the focused workspace when one is active.
    async fn create_session_row(&self) -> CoreResult<String> {
        let path = self.db_path.clone();
        let session = Session::new_chat(new_id(), DEFAULT_SESSION_TITLE.into(), now_ms());
        let id = session.id.clone();
        tokio::task::spawn_blocking(move || -> Result<(), CoreError> {
            let db = Db::open(&path)?;
            migrations::run(&db.0)?;
            let active_ws =
                repos::workspace_open_state::find_focused(&db.0)?.map(|r| r.workspace_id).unwrap_or_default();
            repos::sessions::insert(&db.0, &session)?;
            if !active_ws.is_empty() {
                repos::sessions::set_workspace_id(&db.0, &session.id, &active_ws)?;
            }
            Ok(())
        })
        .await
        .map_err(join_err)??;
        Ok(id)
    }

    /// Runs one user task through the Loop Engine, persists the transcript
    /// as append-only session events, and extends the in-memory history.
    pub async fn run_task(&self, task: &str) -> CoreResult<LoopRunResult> {
        // Lazy session creation: the first turn materializes the session
        // row instead of boot doing it unconditionally.
        let session_id = self.ensure_session_id().await?;
        let mut state = self.state.lock().await;
        // One ordinal per session (not per SessionState) so it survives
        // resume/switch and matches the isolated session path below.
        let delta_counter = self.delta_counter(&session_id).await;
        state.delta_seq = delta_counter.clone();

        let history = std::mem::take(&mut state.history);
        let result = self
            .run_turn(&session_id, history, task, delta_counter.clone(), None, None, None, None, None)
            .await?;
        state.history = result.transcript.clone();
        Ok(result)
    }

    /// Returns the live-delta ordinal counter for `session_id`, creating it
    /// on first use. Shared across turns so the ordinal keeps increasing for
    /// the whole session (consumers dedupe/order `session.delta` by it).
    async fn delta_counter(&self, session_id: &str) -> Arc<AtomicU64> {
        let mut map = self.delta_seqs.lock().await;
        map.entry(session_id.to_string())
            .or_insert_with(|| Arc::new(AtomicU64::new(0)))
            .clone()
    }

    /// Runs a task in an isolated session context — loads history from DB,
    /// runs the Loop Engine without holding the global state lock, and
    /// persists results to the specified session. Allows concurrent runs
    /// in different sessions (D1, conversation-ux-plan §2.4).
    ///
    /// `cancel` is a cooperative stop signal: the loop returns at the next
    /// step boundary (an in-flight provider stream is not aborted mid-token)
    /// and whatever was produced is persisted before returning.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_task_in_session(
        &self,
        session_id: &str,
        task: &str,
        cancel: Option<CancellationToken>,
        provider: Option<Arc<dyn LlmProvider>>,
        model: Option<String>,
        overlay: Option<RoleOverlay>,
        external_session_id: Option<String>,
        max_history_chars: Option<usize>,
    ) -> CoreResult<LoopRunResult> {
        let session_id = session_id.to_string();
        let task = task.to_string();

        let path = self.db_path.clone();
        let sid_for_load = session_id.clone();
        let mut history =
            tokio::task::spawn_blocking(move || -> Result<Vec<ChatMessage>, CoreError> {
                let db = Db::open(&path)?;
                repos::sessions::get(&db.0, &sid_for_load)?;
                let events =
                    repos::events::list_by_aggregate(&db.0, "session", &sid_for_load, None)?;
                Ok(rebuild_history(&events))
            })
            .await
            .map_err(join_err)??;

        // Truncate history on CLI binding change (ADR 0012 D7).
        if let Some(max_chars) = max_history_chars {
            history = truncate_history_chars(history, max_chars);
        }

        let delta_counter = self.delta_counter(&session_id).await;
        self.run_turn(
            &session_id,
            history,
            &task,
            delta_counter,
            cancel,
            provider,
            model,
            overlay,
            external_session_id,
        )
        .await
    }

    /// Shared core of [`run_task`] and [`run_task_in_session`]: bridges
    /// deltas onto the bus, runs the loop, persists only the new messages
    /// and republishes them with their authoritative `events.seq`.
    #[allow(clippy::too_many_arguments)]
    async fn run_turn(
        &self,
        session_id: &str,
        history: Vec<ChatMessage>,
        task: &str,
        delta_counter: Arc<AtomicU64>,
        cancel: Option<CancellationToken>,
        provider: Option<Arc<dyn LlmProvider>>,
        model: Option<String>,
        overlay: Option<RoleOverlay>,
        external_session_id: Option<String>,
    ) -> CoreResult<LoopRunResult> {
        let history_len = history.len();
        // Bridge streaming deltas onto the kernel bus so the shell's event
        // bridge can forward them on `event://session/{id}` (ADR-0002).
        //
        // The injected `seq` is a NON-PERSISTENT live-stream ordinal (1-based,
        // per session). It lets the frontend dedupe replayed deltas and
        // restore ordering within one live stream; it has no relationship to
        // the durable `seq` of rows in the `events` table.
        let delta_counter_for_closure = delta_counter.clone();
        let session_id_for_delta = session_id.to_string();
        let user_cb = self.delta_cb.clone();
        let ctx = self.ctx.clone();
        let delta_bridge: Option<DeltaCallback> = Some(Arc::new(move |delta: String| {
            let seq = delta_counter_for_closure.fetch_add(1, Ordering::Relaxed) + 1;
            if let Some(cb) = &user_cb {
                cb(delta.clone());
            }
            ctx.publish(crate::harness::Event::new(
                "session.delta",
                serde_json::json!({ "sessionId": session_id_for_delta, "text": delta, "seq": seq }),
            ));
        }));

        let effective_provider = provider.unwrap_or_else(|| self.provider.clone());
        let effective_model = model.unwrap_or_else(|| self.model.clone());
        let engine = LoopEngine::new(
            effective_provider,
            LoopConfig {
                model: effective_model,
                external_session_id: external_session_id.clone(),
                ..LoopConfig::default()
            },
        )
        .with_delta_callback(delta_bridge);
        // Apply RoleOverlay (D4): system_prompt override + tool allowlist +
        // temperature (via turn hook). Aligns with PipelineExecutor /
        // GroupChatExecutor existing injection points.
        let engine = match &overlay {
            Some(ov) => {
                let engine = engine
                    .with_system_prompt_override(ov.system_prompt.clone())
                    .with_tool_allowlist(ov.tool_allowlist.clone());
                if let Some(temp) = ov.temperature {
                    engine.with_turn_hook(Arc::new(move |_| TurnOverride {
                        temperature: Some(temp),
                        ..Default::default()
                    }) as TurnHook)
                } else {
                    engine
                }
            }
            None => engine,
        };
        let engine = match cancel {
            Some(token) => engine.with_cancel(token),
            None => engine,
        };

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

        // Persist only the new messages — replayed history is already in the
        // append-only log (and injected memory digests are prepended after
        // the history boundary, never before it). The auto-title rides the
        // same persistence phase: one spawn_blocking round-trip,
        // check-then-update so the derived name is written exactly once.
        let auto_title = derive_title(task);
        let fresh = result.transcript.get(history_len..).unwrap_or_default();
        let appended = self
            .persist_transcript(session_id, fresh, &auto_title)
            .await?;

        // Publish what was just persisted (iron rule: persist first, then
        // emit). Each record rides the session channel carrying its
        // AUTHORITATIVE `seq` — the per-aggregate `events`-table seq used for
        // gap recovery via `listEvents(afterSeq)`. Assistant messages
        // additionally carry `deltaTo`: the live-delta high-water mark this
        // answer covers, so the UI can swap buffer→full text in one step.
        let total_deltas = delta_counter.load(Ordering::Relaxed);
        for rec in &appended {
            let mut body = rec.payload.clone();
            if let Some(obj) = body.as_object_mut() {
                obj.insert("sessionId".into(), json!(session_id));
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
        state.session_id = Some(session_id.to_string());
        state.history = history;
        // The ordinal belongs to the session, not to the live stream: keep
        // the same counter across resumes so consumers can still dedupe.
        state.delta_seq = self.delta_counter(session_id).await;
        Ok(())
    }

    /// Starts a fresh session (`/new` in the REPL).
    pub async fn new_session(&self) -> CoreResult<String> {
        let id = self.create_session_row().await?;

        let mut state = self.state.lock().await;
        state.session_id = Some(id.clone());
        state.history = Vec::new();
        // Fresh session ⇒ the live delta ordinal starts at 1.
        state.delta_seq = self.delta_counter(&id).await;
        Ok(id)
    }

    pub async fn list_sessions(&self) -> CoreResult<Vec<Session>> {
        let path = self.db_path.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<Session>, CoreError> {
            let db = Db::open(&path)?;
            migrations::run(&db.0)?;
            let active_ws =
                repos::workspace_open_state::find_focused(&db.0)?.map(|r| r.workspace_id).unwrap_or_default();
            let filter_ws = if active_ws.is_empty() {
                "__migrated__"
            } else {
                &active_ws
            };
            Ok(repos::sessions::list(&db.0, filter_ws, 100)?)
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

/// Derives a session auto-title from the first task input using a
/// length-tiered extractor:
/// - **≤ 40 chars**: trimmed first line, capped at [`TITLE_MAX_CHARS`].
/// - **41–200 chars**: first sentence (split on `。！？.!?`), capped.
/// - **> 200 chars**: top-3 frequency keywords (stopword-filtered), joined
///   with spaces and capped.
///
/// Whitespace-only input maps to `""` (meaning "keep the default title").
fn derive_title(input: &str) -> String {
    let text = input.trim();
    if text.is_empty() {
        return String::new();
    }
    let char_count = text.chars().count();
    if char_count <= TITLE_MAX_CHARS {
        first_line_truncated(text)
    } else if char_count <= TITLE_LONG_THRESHOLD {
        first_sentence_truncated(text)
    } else {
        keyword_title(text)
    }
}

/// First non-empty line, truncated to [`TITLE_MAX_CHARS`] with a trailing `…`.
fn first_line_truncated(text: &str) -> String {
    let first_line = text.lines().next().unwrap_or("").trim_end();
    truncate_with_ellipsis(first_line, TITLE_MAX_CHARS)
}

/// First sentence (up to the first sentence-ending punctuation), truncated.
/// Falls back to [`first_line_truncated`] when no sentence terminator is found.
fn first_sentence_truncated(text: &str) -> String {
    let end = text
        .char_indices()
        .find(|(_, c)| matches!(c, '。' | '！' | '？' | '.' | '!' | '?'));
    let sentence = match end {
        Some((idx, c)) => text[..idx + c.len_utf8()].trim(),
        None => text.lines().next().unwrap_or("").trim_end(),
    };
    if sentence.is_empty() {
        first_line_truncated(text)
    } else {
        truncate_with_ellipsis(sentence, TITLE_MAX_CHARS)
    }
}

/// Top-3 frequency keywords (stopword-filtered, ≥ 2 chars, ≤ 12 chars to
/// avoid whole-sentence tokens), joined with spaces. Falls back to
/// [`first_line_truncated`] when no keywords survive filtering.
fn keyword_title(text: &str) -> String {
    let mut freq: HashMap<&str, usize> = HashMap::new();
    for token in tokenize(text) {
        let len = token.chars().count();
        if !(2..=12).contains(&len) || is_stopword(token) {
            continue;
        }
        *freq.entry(token).or_insert(0) += 1;
    }
    let mut sorted: Vec<(&&str, &usize)> = freq.iter().collect();
    // Highest frequency first; tie-break by shorter token (more keyword-like).
    sorted.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.len().cmp(&b.0.len())));
    let keywords: Vec<&str> = sorted.iter().take(3).map(|(w, _)| **w).collect();
    if keywords.is_empty() {
        first_line_truncated(text)
    } else {
        truncate_with_ellipsis(&keywords.join(" "), TITLE_MAX_CHARS)
    }
}

/// Truncates `s` to `max` characters, appending `…` when truncation happened.
fn truncate_with_ellipsis(s: &str, max: usize) -> String {
    let mut title: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        title.push('…');
    }
    title
}

/// Splits on whitespace and common punctuation, returning non-empty slices.
fn tokenize(text: &str) -> Vec<&str> {
    text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '，' | ',' | '。' | '.' | '！' | '!' | '？' | '?' | '；' | ';' | '：' | ':'
                    | '、' | '/' | '|' | '-' | '_' | '"' | '\'' | '`' | '(' | ')' | '（' | '）'
                    | '【' | '】' | '[' | ']' | '{' | '}'
            )
    })
    .filter(|s| !s.is_empty())
    .collect()
}

/// Common Chinese/English function words and particles that carry no topic
/// signal for keyword extraction.
fn is_stopword(word: &str) -> bool {
    const STOPWORDS: &[&str] = &[
        "the", "a", "an", "and", "or", "but", "if", "then", "else", "for", "of", "to", "in",
        "on", "at", "by", "with", "from", "as", "is", "it", "this", "that", "these", "those",
        "i", "you", "he", "she", "we", "they", "me", "him", "her", "us", "them", "my", "your",
        "his", "its", "our", "their", "what", "which", "who", "when", "where", "why", "how",
        "do", "does", "did", "can", "could", "should", "would", "will", "shall", "may", "might",
        "must", "have", "has", "had", "be", "been", "being", "am", "are", "was", "were", "not",
        "no", "yes", "so", "too", "very", "just", "also", "only", "up", "down", "out", "about",
        "into", "over", "under", "again", "here", "there", "all", "any", "both", "each", "few",
        "more", "most", "other", "some", "such",
        "的", "了", "是", "在", "我", "你", "他", "她", "它", "们", "这", "那", "有", "和", "与",
        "或", "但", "如", "果", "一", "个", "上", "下", "中", "为", "以", "及", "等", "都", "也",
        "就", "还", "不", "没", "要", "会", "能", "可", "对", "让", "把", "被", "给", "向", "从",
        "到", "于", "之", "其", "而", "且", "并", "则", "若", "虽", "然", "因", "所", "吗", "呢",
        "吧", "啊", "呀", "哦", "嗯",
    ];
    STOPWORDS.contains(&word)
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

/// Truncates `history` to the last `max_chars` characters of content,
/// keeping message boundaries intact (ADR 0012 D7). When the total fits,
/// the history is returned unchanged.
fn truncate_history_chars(history: Vec<ChatMessage>, max_chars: usize) -> Vec<ChatMessage> {
    let total: usize = history.iter().map(|m| m.content.chars().count()).sum();
    if total <= max_chars {
        return history;
    }
    let mut kept = Vec::new();
    let mut accumulated = 0usize;
    for msg in history.into_iter().rev() {
        let len = msg.content.chars().count();
        if accumulated + len > max_chars {
            break;
        }
        accumulated += len;
        kept.push(msg);
    }
    kept.reverse();
    kept
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
            repos::events::list_by_aggregate(&conn, "session", &kernel.session_id().await.unwrap(), None)
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
        let session_id = first.ensure_session_id().await.unwrap();
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
        assert_eq!(sessions.len(), 0);

        let new_id = kernel.new_session().await.unwrap();
        let sessions = kernel.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 1);
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
            // Medium text (41-200 chars): first sentence extraction.
            ("hello world. this is a longer test sentence exceeding forty.", "hello world."),
            ("这是一个测试。后面跟着足够多的填充文字来确保总长度超过四十个字符才行所以多写一些。", "这是一个测试。"),
            ("first sentence here? second one continues past forty chars!", "first sentence here?"),
        ];
        for (input, expected) in cases {
            assert_eq!(derive_title(input), expected, "input: {input:?}");
        }
    }

    #[test]
    fn derive_title_long_text_uses_keywords() {
        // > 200 chars: keyword extraction path.
        let long_en = format!(
            "{}{}{}",
            "alpha ".repeat(5),
            "beta ".repeat(3),
            "gamma ".repeat(2),
        ) + &"x ".repeat(100);
        let title = derive_title(&long_en);
        assert!(!title.is_empty(), "long text should yield a title");
        assert!(
            title.chars().count() <= TITLE_MAX_CHARS + 1,
            "title must fit within max chars (plus ellipsis): got {title:?}"
        );
        // alpha (5x) should surface before beta (3x) / gamma (2x).
        assert!(title.contains("alpha"), "top keyword alpha should appear: {title:?}");

        let long_cjk = "错误 ".repeat(60);
        let title_cjk = derive_title(&long_cjk);
        assert!(!title_cjk.is_empty(), "long CJK text should yield a title");
        assert!(
            title_cjk.chars().count() <= TITLE_MAX_CHARS + 1,
            "CJK title must fit: got {title_cjk:?}"
        );
    }

    #[test]
    fn truncate_history_chars_returns_unchanged_when_within_budget() {
        let history = vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant("world"),
        ];
        let truncated = truncate_history_chars(history.clone(), 100);
        assert_eq!(truncated, history);
    }

    #[test]
    fn truncate_history_chars_drops_oldest_messages() {
        let history = vec![
            ChatMessage::user("aaaa"),   // 4
            ChatMessage::assistant("bbbb"), // 4
            ChatMessage::user("cccc"),   // 4
            ChatMessage::assistant("dddd"), // 4
        ];
        // Budget 10 → keep last 2 messages (8 chars), 3rd would exceed (12).
        let truncated = truncate_history_chars(history, 10);
        assert_eq!(truncated.len(), 2);
        assert_eq!(truncated[0].content, "cccc");
        assert_eq!(truncated[1].content, "dddd");
    }

    #[test]
    fn truncate_history_chars_empty_budget_yields_empty() {
        let history = vec![ChatMessage::user("x")];
        let truncated = truncate_history_chars(history, 0);
        assert!(truncated.is_empty());
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
        let sid = kernel.ensure_session_id().await.unwrap();

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
        let sid = kernel.ensure_session_id().await.unwrap();
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
