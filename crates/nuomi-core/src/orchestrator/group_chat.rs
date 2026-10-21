//! Group-chat executor: Handoff protocol + pluggable Selector (SPEC AC9a–AC9e).
//!
//! Termination guarantees (AC9c): max rounds cap, selector convergence signal,
//! and handoff hop counting that rejects loops (`A→B→A` revisits beyond
//! `max_hops` fail with [`OrchestratorError::HandoffLoopDetected`]).

use std::sync::Arc;

use serde_json::{json, Value};

use crate::domain::{AutoRemember, RoleMemoryConfig, Team};
use crate::plugins::MemoryService;
use crate::providers::{
    ChatMessage, ChatRequest, ChatResponse, LlmProvider, StreamEvent, ToolCall, ToolDef,
};

use super::input::{recorded_cli_handle, ProviderResolver, RecordedCliHandle, TeamRunInput};
use super::selector::{GroupMember, GroupState};
use super::whiteboard::{record_selector_decision, record_turn, WhiteBoardService};
use super::OrchestratorError;

/// Internal tool the model calls to hand control to another member.
pub const HANDOFF_TOOL: &str = "handoff_to_next";

/// Live-stream sink for one speaker turn: `(role_id, role_name, delta)`.
/// Wired by the caller (team_runner) to publish `session.delta` events so
/// group-chat views render streaming utterances per agent (SPEC 5.8.1-4).
pub type GroupDeltaBridge = Arc<dyn Fn(&str, &str, &str) + Send + Sync>;

fn handoff_tool_def() -> ToolDef {
    ToolDef {
        name: HANDOFF_TOOL.into(),
        description: "Hand the conversation to another participant.".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "target": { "type": "string", "description": "Participant name" },
                "message": { "type": "string", "description": "Why you hand off" }
            },
            "required": ["target"]
        }),
    }
}

/// Limits resolved from `team.config` with sane defaults.
#[derive(Debug, Clone)]
pub struct GroupChatConfig {
    pub max_rounds: usize,
    /// How many times the same agent may speak in a row before the executor
    /// forces a speaker change.
    pub max_consecutive: usize,
    /// How many times any single member may *receive* a handoff before the
    /// transfer chain is declared a loop.
    pub max_hops: u32,
}

impl GroupChatConfig {
    pub fn of(team: &Team) -> Self {
        let cfg = &team.config;
        Self {
            max_rounds: cfg.get("max_rounds").and_then(Value::as_u64).unwrap_or(6) as usize,
            max_consecutive: cfg
                .get("max_consecutive")
                .and_then(Value::as_u64)
                .unwrap_or(2) as usize,
            max_hops: cfg.get("max_hops").and_then(Value::as_u64).unwrap_or(4) as u32,
        }
    }
}

/// Outcome of a group-chat run.
#[derive(Debug, Clone)]
pub struct GroupChatOutcome {
    pub transcript: Vec<super::selector::GroupTurn>,
    /// True if the discussion ended via the convergence signal rather than
    /// exhausting `max_rounds`.
    pub converged: bool,
    pub rounds: usize,
    /// CLI Agent session handles observed during the run (ADR 0012 D8) —
    /// team_runner upserts them into `session_cli_handles` at settlement.
    pub cli_handles: Vec<RecordedCliHandle>,
}

/// Drives [`crate::domain::TeamTopology::GroupChat`] teams.
pub struct GroupChatExecutor {
    selector: Arc<dyn super::selector::SpeakerSelector>,
    /// Per-agent long-term memory injected into each speaker's context and
    /// written after each utterance when the role opts in (ADR 0019 extended
    /// to group chat). `None` keeps the legacy no-memory behaviour.
    memory: Option<MemoryService>,
    /// Optional live-stream sink: every text delta of the current speaker is
    /// forwarded here before the utterance completes.
    delta_bridge: Option<GroupDeltaBridge>,
}

impl GroupChatExecutor {
    pub fn new(selector: Arc<dyn super::selector::SpeakerSelector>) -> Self {
        Self {
            selector,
            memory: None,
            delta_bridge: None,
        }
    }

    /// Attaches a long-term memory service so each speaker recalls its own
    /// per-agent memories and persists utterances when configured.
    pub fn with_memory(mut self, memory: MemoryService) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Attaches a live-stream sink for per-agent deltas (SPEC 5.8.1-4).
    pub fn with_delta_bridge(mut self, bridge: GroupDeltaBridge) -> Self {
        self.delta_bridge = Some(bridge);
        self
    }

    pub async fn run(
        &self,
        input: &TeamRunInput,
        providers: &ProviderResolver,
        wb: &WhiteBoardService,
    ) -> Result<GroupChatOutcome, OrchestratorError> {
        let config = GroupChatConfig::of(&input.team);
        let members = input
            .team
            .member_role_ids
            .iter()
            .map(|rid| {
                input.require_role(rid).map(|r| GroupMember {
                    role_id: r.id.clone(),
                    name: r.name.clone(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut state = GroupState::new(members);

        let mut handoff_receipts = vec![0u32; state.members.len()];
        let mut pending_handoff: Option<usize> = None;
        let mut converged = false;
        let mut rounds = 0usize;
        let mut cli_handles = Vec::new();

        while rounds < config.max_rounds {
            // ---- pick the next speaker ------------------------------------
            let speaker = match pending_handoff.take() {
                Some(target) => target, // explicit handoff wins over the selector
                None => {
                    let chosen = self.selector.select(&state).await;
                    let converged_signal = chosen == state.members.len();
                    // Record the selector decision as an append-only event so
                    // group-chat speaker selection is auditable and replayable.
                    let candidates: Vec<String> =
                        state.members.iter().map(|m| m.role_id.clone()).collect();
                    let chosen_role_id = if converged_signal || chosen >= state.members.len() {
                        None
                    } else {
                        Some(state.members[chosen].role_id.as_str())
                    };
                    let chosen_role_name = if converged_signal || chosen >= state.members.len() {
                        None
                    } else {
                        Some(state.members[chosen].name.as_str())
                    };
                    let _ = record_selector_decision(
                        wb,
                        &input.session_id,
                        self.selector.strategy_name(),
                        candidates,
                        chosen,
                        chosen_role_id,
                        chosen_role_name,
                        converged_signal,
                    )
                    .await;
                    if converged_signal {
                        converged = true; // convergence signal (index == len)
                        break;
                    }
                    if chosen > state.members.len() {
                        return Err(OrchestratorError::InvalidTeam(format!(
                            "selector returned out-of-range index {chosen}"
                        )));
                    }
                    // Consecutive-speech constraint: force a change when the
                    // same agent has held the floor for too long.
                    if state.last_speaker == Some(chosen)
                        && state.consecutive_count >= config.max_consecutive
                        && state.members.len() > 1
                    {
                        (chosen + 1) % state.members.len()
                    } else {
                        chosen
                    }
                }
            };

            // ---- one utterance --------------------------------------------
            let role = input.require_role(&state.members[speaker].role_id)?;
            let provider = providers.resolve(role);
            let request = Self::build_request(input, role, &state, wb, &self.memory).await?;
            let response = Self::stream_turn(&provider, &request, role, self.delta_bridge.as_ref())
                .await
                .map_err(|e| OrchestratorError::Provider(e.to_string()))?;
            // Non-SSE endpoints answer a streaming request with an empty SSE
            // body (no `data:` lines → no deltas, empty Completed). Fall back
            // to one-shot completion so utterances never go silent — parity
            // with the pre-streaming behaviour.
            let response = if response.content.is_empty() && response.tool_calls.is_empty() {
                provider
                    .complete(&request)
                    .await
                    .map_err(|e| OrchestratorError::Provider(e.to_string()))?
            } else {
                response
            };

            state.push_turn(speaker, response.content.clone());
            if let Some(handle) = recorded_cli_handle(role, &response) {
                cli_handles.push(handle);
            }
            record_turn(
                wb,
                &input.session_id,
                role,
                "message",
                &response.content,
                None,
                None,
            )
            .await?;

            // Per-agent memory write (ADR 0019 extended to group chat): when
            // the speaking role opts into auto_remember, persist its own
            // utterance as a private memory entry for future recall.
            if let Some(ref mem) = self.memory {
                if let Some(mc) = role
                    .params
                    .get("memory")
                    .and_then(|v| serde_json::from_value::<RoleMemoryConfig>(v.clone()).ok())
                {
                    if mc.enabled && matches!(mc.auto_remember, AutoRemember::UserInputOnly) {
                        let mem = mem.clone();
                        let sid = input.session_id.clone();
                        let tags = mc.tags.clone();
                        let rid = role.id.clone();
                        let text = response.content.clone();
                        tokio::spawn(async move {
                            let _ = mem
                                .remember(
                                    text,
                                    Some(sid),
                                    tags,
                                    "group_chat_utterance",
                                    false,
                                    Some(rid),
                                )
                                .await;
                        });
                    }
                }
            }

            // ---- handoff protocol -----------------------------------------
            if let Some(call) = response
                .tool_calls
                .iter()
                .find(|c: &&ToolCall| c.name == HANDOFF_TOOL)
            {
                let target_name = call
                    .arguments
                    .get("target")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let target = state
                    .members
                    .iter()
                    .position(|m| m.name.eq_ignore_ascii_case(target_name))
                    .ok_or_else(|| OrchestratorError::MemberNotFound {
                        team: input.team.name.clone(),
                        member: target_name.to_string(),
                    })?;

                handoff_receipts[target] += 1;
                if handoff_receipts[target] > config.max_hops {
                    let chain = state
                        .turns
                        .iter()
                        .map(|t| state.members[t.speaker].name.as_str())
                        .collect::<Vec<_>>()
                        .join("→");
                    return Err(OrchestratorError::HandoffLoopDetected {
                        chain,
                        max: config.max_hops,
                    });
                }
                pending_handoff = Some(target);
            }

            rounds += 1;
            state.round = rounds;
        }

        Ok(GroupChatOutcome {
            transcript: state.turns,
            converged,
            rounds,
            cli_handles,
        })
    }

    /// Consumes one speaker turn as a live stream: every `TextDelta` is
    /// forwarded to the delta bridge (live `session.delta` mirroring) and the
    /// terminal `Completed` event yields the assembled response. If a
    /// provider ends its stream without `Completed`, the deltas collected so
    /// far are assembled defensively.
    async fn stream_turn(
        provider: &Arc<dyn LlmProvider>,
        request: &ChatRequest,
        role: &crate::domain::Role,
        bridge: Option<&GroupDeltaBridge>,
    ) -> Result<ChatResponse, crate::providers::ProviderError> {
        use futures::StreamExt;
        let mut stream = provider.stream(request);
        let mut assembled = String::new();
        while let Some(event) = stream.next().await {
            match event? {
                StreamEvent::TextDelta(delta) => {
                    assembled.push_str(&delta);
                    if let Some(cb) = bridge {
                        cb(&role.id, &role.name, &delta);
                    }
                }
                StreamEvent::Completed(response) => return Ok(response),
            }
        }
        Ok(ChatResponse {
            content: assembled,
            ..ChatResponse::default()
        })
    }

    /// Builds the per-turn request: role system prompt, shared transcript and
    /// whiteboard digest as user context, plus the handoff tool. When a memory
    /// service is attached, the speaking role's per-agent memories are recalled
    /// and appended to the system prompt (ADR 0019 extended to group chat).
    async fn build_request(
        input: &TeamRunInput,
        role: &crate::domain::Role,
        state: &GroupState,
        wb: &WhiteBoardService,
        memory: &Option<MemoryService>,
    ) -> Result<ChatRequest, OrchestratorError> {
        let board_digest = wb
            .to_context_digest(&input.session_id)
            .await
            .map_err(|e| OrchestratorError::Store(e.to_string()))?;

        let mut convo = format!("Task:\n{}\n\nDiscussion so far:\n", input.task);
        for turn in &state.turns {
            convo.push_str(&format!(
                "- {}: {}\n",
                state.members[turn.speaker].name, turn.text
            ));
        }
        if state.turns.is_empty() {
            convo.push_str("(you speak first)\n");
        }
        if !board_digest.is_empty() {
            convo.push_str(&format!("\nWhiteBoard notes:\n{board_digest}\n"));
        }
        convo.push_str(&format!(
            "\nYou are {}. Call {} to pass the floor, or answer without it.",
            role.name, HANDOFF_TOOL
        ));

        let mut system = role
            .system_prompt_override
            .clone()
            .unwrap_or_else(|| format!("You are {} in a group discussion.", role.name));

        // Per-agent memory recall: inject the speaker's private + global
        // memories into the system prompt so each agent brings its own
        // cross-session knowledge to the discussion.
        if let Some(mem) = memory {
            let keyword = input.task.split_whitespace().next().map(str::to_string);
            if let Ok(hits) = mem.recall(keyword, None, Some(role.id.clone()), 5).await {
                if !hits.is_empty() {
                    let digest = hits
                        .iter()
                        .map(|m| format!("- {}", m.content))
                        .collect::<Vec<_>>()
                        .join("\n");
                    system.push_str(&format!("\n\nRelevant long-term memories:\n{digest}"));
                }
            }
        }

        Ok(ChatRequest {
            model: input.model.clone(),
            system_prompt: Some(system),
            messages: vec![ChatMessage::user(convo)],
            tools: vec![handoff_tool_def()],
            temperature: role.temperature,
            max_tokens: role.max_tokens,
            cache_retention: Default::default(),
            cache_scope: None,
            // Resume the speaker's own CLI session when it ran before
            // (ADR 0012 D8); non-CLI providers ignore the field.
            external_session_id: input.cli_handles.get(&role.id).cloned(),
        })
    }
}
