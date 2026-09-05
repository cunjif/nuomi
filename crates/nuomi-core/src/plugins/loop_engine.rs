//! Loop Engine: the ReAct executor (thought → tool_call → tool_result …).

use std::sync::Arc;

use futures::StreamExt;

use crate::harness::{Context, Event, HarnessError, Plugin};
use crate::providers::{ChatMessage, ChatRequest, LlmProvider, StreamEvent};
use async_trait::async_trait;

use super::hooks::{HookDecision, HookPoint, HookRegistry};
use super::tools::ToolRegistry;
use super::MemoryService;

/// Limits and knobs for a loop run.
#[derive(Debug, Clone)]
pub struct LoopConfig {
    pub model: String,
    pub max_steps: usize,
    /// Memory keywords injected at session start (tag filter).
    pub memory_tag: Option<String>,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            model: "default".into(),
            max_steps: 16,
            memory_tag: None,
        }
    }
}

/// Outcome of one loop run.
#[derive(Debug)]
pub struct LoopRunResult {
    pub transcript: Vec<ChatMessage>,
    pub final_text: String,
    pub steps: usize,
    /// True if `max_steps` was exhausted without a final answer.
    pub truncated: bool,
}

/// Streaming delta observer (e.g. CLI stdout printer).
pub type DeltaCallback = Arc<dyn Fn(String) + Send + Sync>;

/// Drives one agent's ReAct loop over a provider + tools + hooks.
pub struct LoopEngine {
    provider: Arc<dyn LlmProvider>,
    config: LoopConfig,
    on_delta: Option<DeltaCallback>,
}

impl LoopEngine {
    pub fn new(provider: Arc<dyn LlmProvider>, config: LoopConfig) -> Self {
        Self {
            provider,
            config,
            on_delta: None,
        }
    }

    /// Sets an optional callback invoked for every streamed text delta
    /// (used by the headless CLI to echo tokens to the terminal).
    pub fn with_delta_callback(mut self, cb: Option<DeltaCallback>) -> Self {
        self.on_delta = cb;
        self
    }

    async fn stream_once(
        &self,
        request: &ChatRequest,
    ) -> Result<crate::providers::ChatResponse, HarnessError> {
        let mut stream = self.provider.stream(request);
        let mut final_resp = None;
        while let Some(item) = stream.next().await {
            match item {
                Ok(StreamEvent::TextDelta(delta)) => {
                    if let Some(cb) = &self.on_delta {
                        cb(delta);
                    }
                    // Deltas are for UI streaming; the assembled response
                    // arrives in Completed.
                }
                Ok(StreamEvent::Completed(resp)) => final_resp = Some(resp),
                Err(e) => {
                    return Err(HarnessError::PluginFailed {
                        plugin: "loop_engine".into(),
                        phase: "provider_stream",
                        message: e.to_string(),
                    })
                }
            }
        }
        final_resp.ok_or_else(|| HarnessError::PluginFailed {
            plugin: "loop_engine".into(),
            phase: "provider_stream",
            message: "stream ended without Completed event".into(),
        })
    }

    /// Runs the loop until the model answers without tool calls or limits hit.
    pub async fn run(
        &self,
        ctx: &Context,
        tools: &ToolRegistry,
        hooks: Option<&HookRegistry>,
        memory: Option<&MemoryService>,
        user_task: &str,
    ) -> Result<LoopRunResult, HarnessError> {
        self.run_with_history(ctx, tools, hooks, memory, Vec::new(), user_task)
            .await
    }

    /// Like [`run`], but seeds the transcript with a previously persisted
    /// conversation (session resume). `history` must already be ordered and
    /// must not end with a dangling tool call.
    pub async fn run_with_history(
        &self,
        ctx: &Context,
        tools: &ToolRegistry,
        hooks: Option<&HookRegistry>,
        memory: Option<&MemoryService>,
        history: Vec<ChatMessage>,
        user_task: &str,
    ) -> Result<LoopRunResult, HarnessError> {
        ctx.publish(Event::new(
            "session.start",
            serde_json::json!({ "task": user_task }),
        ));

        let system_prompt = resolve_system_prompt(ctx).await?;
        let allowlist: Vec<String> = Vec::new(); // role-level filtering applied by orchestrator
        let defs = tools.defs_for(&allowlist).await;

        let mut transcript = history;
        transcript.push(ChatMessage::user(user_task.to_string()));

        // Memory injection: recall by tag / first words of the task.
        if let Some(mem) = memory {
            let keyword = user_task.split_whitespace().next().map(str::to_string);
            if let Ok(hits) = mem.recall(keyword, self.config.memory_tag.clone(), 5).await {
                if !hits.is_empty() {
                    let digest = hits
                        .iter()
                        .map(|m| format!("- {}", m.content))
                        .collect::<Vec<_>>()
                        .join("\n");
                    transcript.insert(
                        0,
                        ChatMessage::system(format!("Relevant long-term memories:\n{digest}")),
                    );
                }
            }
        }

        let mut steps = 0usize;
        while steps < self.config.max_steps {
            steps += 1;
            let request = ChatRequest {
                model: self.config.model.clone(),
                system_prompt: Some(system_prompt.clone()),
                messages: transcript.clone(),
                tools: defs.clone(),
                temperature: None,
                max_tokens: None,
                cache_retention: Default::default(),
            };
            let response = self.stream_once(&request).await?;
            // A length-capped response means emitted tool arguments were cut
            // mid-JSON; executing them would act on corrupt input.
            let output_truncated = response.finish_reason.as_deref() == Some("length");

            if response.tool_calls.is_empty() {
                transcript.push(ChatMessage::assistant(response.content.clone()));
                ctx.publish(Event::new(
                    "session.end",
                    serde_json::json!({ "steps": steps }),
                ));
                return Ok(LoopRunResult {
                    transcript,
                    final_text: response.content,
                    steps,
                    truncated: false,
                });
            }

            let mut assistant = ChatMessage::assistant(response.content.clone());
            assistant.tool_calls = response.tool_calls.clone();
            transcript.push(assistant);

            for call in response.tool_calls {
                if output_truncated {
                    transcript.push(ChatMessage::tool_result(
                        call.id,
                        "[tool call skipped] output truncated; arguments may be incomplete",
                    ));
                    continue;
                }
                let payload = serde_json::json!({ "tool": call.name, "arguments": call.arguments });
                let mut denied: Option<String> = None;
                if let Some(hook_reg) = hooks {
                    let decision = hook_reg.run(HookPoint::PreToolCall, &payload).await;
                    ctx.publish(HookRegistry::audit_event(
                        HookPoint::PreToolCall,
                        &payload,
                        &decision,
                    ));
                    if let HookDecision::Deny(reason) = decision {
                        denied = Some(reason);
                    }
                }
                ctx.publish(super::tools::ToolRegistry::event(
                    &call.name,
                    &call.arguments,
                ));

                let result = match denied {
                    Some(reason) => format!("[denied by hook] {reason}"),
                    None => match tools.execute(&call.name, &call.arguments).await {
                        Ok(out) => out,
                        Err(e) => format!("[tool error] {e}"),
                    },
                };
                if let Some(hook_reg) = hooks {
                    let payload = serde_json::json!({ "tool": call.name, "result": result });
                    let decision = hook_reg.run(HookPoint::PostToolCall, &payload).await;
                    ctx.publish(HookRegistry::audit_event(
                        HookPoint::PostToolCall,
                        &payload,
                        &decision,
                    ));
                }
                transcript.push(ChatMessage::tool_result(call.id, result));
            }
        }

        Ok(LoopRunResult {
            transcript,
            final_text: String::new(),
            steps,
            truncated: true,
        })
    }
}

async fn resolve_system_prompt(ctx: &Context) -> Result<String, HarnessError> {
    if let Some(svc) = ctx
        .service::<super::SystemPromptService>("system_prompt")
        .await
    {
        return svc.active().await.map_err(|e| HarnessError::PluginFailed {
            plugin: "system_prompt".into(),
            phase: "resolve",
            message: e.to_string(),
        });
    }
    Ok("You are nuomi, a helpful agent.".to_string())
}

/// The plugin wrapper. Holds provider/config wiring done at boot time.
pub struct LoopEnginePlugin {
    engine: Arc<LoopEngine>,
}

impl LoopEnginePlugin {
    pub fn new(engine: LoopEngine) -> Self {
        Self {
            engine: Arc::new(engine),
        }
    }
}

#[async_trait]
impl Plugin for LoopEnginePlugin {
    fn id(&self) -> &str {
        "loop_engine"
    }

    async fn init(&self, ctx: &Context) -> Result<(), HarnessError> {
        ctx.register_service("loop_engine", "", self.engine.clone())
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{FakeLlm, MessageRole, ToolCall, ToolDef};
    use async_trait::async_trait;
    use serde_json::json;

    struct AddTool;

    #[async_trait]
    impl crate::plugins::tools::Tool for AddTool {
        fn def(&self) -> ToolDef {
            ToolDef {
                name: "add".into(),
                description: "adds".into(),
                parameters: json!({}),
            }
        }
        async fn execute(&self, args: &serde_json::Value) -> Result<String, HarnessError> {
            let a = args["a"].as_i64().unwrap_or(0);
            let b = args["b"].as_i64().unwrap_or(0);
            Ok((a + b).to_string())
        }
    }

    fn tool_call_response() -> crate::providers::ChatResponse {
        crate::providers::ChatResponse {
            content: String::new(),
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                name: "add".into(),
                arguments: json!({ "a": 1, "b": 2 }),
            }],
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn react_loop_executes_tool_then_answers() {
        let provider = Arc::new(FakeLlm::new(
            "fake",
            vec![tool_call_response(), FakeLlm::response("the sum is 3")],
        ));
        let ctx = Context::default();
        let tools = ToolRegistry::new();
        tools.register(Arc::new(AddTool)).await.unwrap();
        let engine = LoopEngine::new(provider, LoopConfig::default());
        let result = engine
            .run(&ctx, &tools, None, None, "compute 1+2")
            .await
            .unwrap();
        assert_eq!(result.final_text, "the sum is 3");
        assert_eq!(result.steps, 2);
        assert!(!result.truncated);
        // transcript contains the tool result message
        assert!(result
            .transcript
            .iter()
            .any(|m| m.role == MessageRole::Tool && m.content == "3"));
    }

    #[tokio::test]
    async fn pre_tool_hook_can_deny_and_audit() {
        let provider = Arc::new(FakeLlm::new(
            "fake",
            vec![
                tool_call_response(),
                FakeLlm::response("ok, denied earlier"),
            ],
        ));
        let ctx = Context::default();
        let tools = ToolRegistry::new();
        tools.register(Arc::new(AddTool)).await.unwrap();

        let hooks = HookRegistry::new();
        hooks
            .add(HookPoint::PreToolCall, 0, |payload| {
                Box::pin(async move {
                    if payload["tool"] == "add" {
                        HookDecision::Deny("not allowed in test".into())
                    } else {
                        HookDecision::Allow
                    }
                })
            })
            .await;

        let engine = LoopEngine::new(provider, LoopConfig::default());
        let result = engine
            .run(&ctx, &tools, Some(&hooks), None, "compute")
            .await
            .unwrap();
        assert!(result
            .transcript
            .iter()
            .any(|m| m.role == MessageRole::Tool && m.content.contains("denied by hook")));
    }

    #[tokio::test]
    async fn max_steps_exhaustion_marks_truncated() {
        // Always answers with a tool call → never converges.
        let provider = Arc::new(FakeLlm::new("fake", vec![]));
        provider.extend_script(std::iter::repeat_n(tool_call_response(), 3));

        let ctx = Context::default();
        let tools = ToolRegistry::new();
        tools.register(Arc::new(AddTool)).await.unwrap();
        let engine = LoopEngine::new(
            provider,
            LoopConfig {
                model: "m".into(),
                max_steps: 3,
                memory_tag: None,
            },
        );
        let result = engine
            .run(&ctx, &tools, None, None, "loop forever")
            .await
            .unwrap();
        assert!(result.truncated);
        assert_eq!(result.steps, 3);
    }

    #[tokio::test]
    async fn memory_is_injected_as_system_message() {
        let dir = tempfile::tempdir().unwrap();
        let db_path: Arc<str> = Arc::from(dir.path().join("t.db").to_string_lossy().to_string());
        let mem = MemoryService::new(db_path.clone());
        mem.remember("user prefers rust".into(), None, vec![], "note", false)
            .await
            .unwrap();

        let provider = Arc::new(FakeLlm::new("fake", vec![FakeLlm::response("done")]));
        let ctx = Context::default();
        let tools = ToolRegistry::new();
        let engine = LoopEngine::new(provider.clone(), LoopConfig::default());
        let _ = engine
            .run(&ctx, &tools, None, Some(&mem), "rust question")
            .await
            .unwrap();
        let reqs = provider.requests.lock().unwrap();
        assert!(!reqs.is_empty());
        let has_memory = reqs[0]
            .messages
            .iter()
            .any(|m| m.role == MessageRole::System && m.content.contains("prefers rust"));
        assert!(has_memory);
    }

    #[tokio::test]
    async fn delta_callback_receives_streamed_text() {
        let provider = Arc::new(FakeLlm::new(
            "fake",
            vec![FakeLlm::response("streamed answer")],
        ));
        let seen: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = seen.clone();
        let engine = LoopEngine::new(provider, LoopConfig::default()).with_delta_callback(Some(
            Arc::new(move |delta: String| sink.lock().unwrap().push(delta)),
        ));
        let ctx = Context::default();
        let tools = ToolRegistry::new();
        let result = engine
            .run(&ctx, &tools, None, None, "say hi")
            .await
            .unwrap();
        assert_eq!(result.final_text, "streamed answer");
        assert_eq!(*seen.lock().unwrap(), vec!["streamed answer".to_string()]);
    }

    #[tokio::test]
    async fn length_capped_response_skips_tool_execution() {
        let mut truncated = tool_call_response();
        truncated.finish_reason = Some("length".into());
        let provider = Arc::new(FakeLlm::new(
            "fake",
            vec![truncated, FakeLlm::response("recovered")],
        ));
        let ctx = Context::default();
        let tools = ToolRegistry::new();
        tools.register(Arc::new(AddTool)).await.unwrap();
        let engine = LoopEngine::new(provider, LoopConfig::default());
        let result = engine
            .run(&ctx, &tools, None, None, "compute 1+2")
            .await
            .unwrap();
        assert_eq!(result.final_text, "recovered");
        let tool_msg = result
            .transcript
            .iter()
            .find(|m| m.role == MessageRole::Tool)
            .expect("tool result present");
        assert_eq!(
            tool_msg.content,
            "[tool call skipped] output truncated; arguments may be incomplete"
        );
    }

    #[tokio::test]
    async fn history_is_seeded_into_transcript_and_request() {
        let provider = Arc::new(FakeLlm::new("fake", vec![FakeLlm::response("continued")]));
        let engine = LoopEngine::new(provider.clone(), LoopConfig::default());
        let ctx = Context::default();
        let tools = ToolRegistry::new();
        let history = vec![
            ChatMessage::user("earlier question"),
            ChatMessage::assistant("earlier answer"),
        ];
        let result = engine
            .run_with_history(&ctx, &tools, None, None, history, "follow up")
            .await
            .unwrap();
        assert_eq!(result.final_text, "continued");
        // Rebuilt transcript keeps the resumed prefix before the new turn.
        assert_eq!(result.transcript[0].content, "earlier question");
        assert_eq!(result.transcript[1].content, "earlier answer");
        assert_eq!(result.transcript[2].role, MessageRole::User);

        let reqs = provider.requests.lock().unwrap();
        let sent: Vec<&str> = reqs[0]
            .messages
            .iter()
            .map(|m| m.content.as_str())
            .collect();
        assert_eq!(
            sent,
            vec!["earlier question", "earlier answer", "follow up"]
        );
    }
}
