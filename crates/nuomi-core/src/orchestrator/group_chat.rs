//! Group-chat executor: Handoff protocol + pluggable Selector (SPEC AC9a–AC9e).
//!
//! Termination guarantees (AC9c): max rounds cap, selector convergence signal,
//! and handoff hop counting that rejects loops (`A→B→A` revisits beyond
//! `max_hops` fail with [`OrchestratorError::HandoffLoopDetected`]).

use std::sync::Arc;

use serde_json::{json, Value};

use crate::domain::Team;
use crate::providers::{ChatMessage, ChatRequest, ToolCall, ToolDef};

use super::input::{ProviderResolver, TeamRunInput};
use super::selector::{GroupMember, GroupState};
use super::whiteboard::{record_turn, WhiteBoardService};
use super::OrchestratorError;

/// Internal tool the model calls to hand control to another member.
pub const HANDOFF_TOOL: &str = "handoff_to_next";

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
}

/// Drives [`crate::domain::TeamTopology::GroupChat`] teams.
pub struct GroupChatExecutor {
    selector: Arc<dyn super::selector::SpeakerSelector>,
}

impl GroupChatExecutor {
    pub fn new(selector: Arc<dyn super::selector::SpeakerSelector>) -> Self {
        Self { selector }
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

        while rounds < config.max_rounds {
            // ---- pick the next speaker ------------------------------------
            let speaker = match pending_handoff.take() {
                Some(target) => target, // explicit handoff wins over the selector
                None => {
                    let chosen = self.selector.select(&state).await;
                    if chosen == state.members.len() {
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
            let request = Self::build_request(input, role, &state, wb).await?;
            let response = provider
                .complete(&request)
                .await
                .map_err(|e| OrchestratorError::Provider(e.to_string()))?;

            state.push_turn(speaker, response.content.clone());
            record_turn(wb, &input.session_id, role, "message", &response.content).await?;

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
        })
    }

    /// Builds the per-turn request: role system prompt, shared transcript and
    /// whiteboard digest as user context, plus the handoff tool.
    async fn build_request(
        input: &TeamRunInput,
        role: &crate::domain::Role,
        state: &GroupState,
        wb: &WhiteBoardService,
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

        let system = role
            .system_prompt_override
            .clone()
            .unwrap_or_else(|| format!("You are {} in a group discussion.", role.name));

        Ok(ChatRequest {
            model: input.model.clone(),
            system_prompt: Some(system),
            messages: vec![ChatMessage::user(convo)],
            tools: vec![handoff_tool_def()],
            temperature: role.temperature,
            max_tokens: role.max_tokens,
            cache_retention: Default::default(),
            cache_scope: None,
        })
    }
}
