//! Pluggable speaker selection for group chats (SPEC D6', AC9b).
//!
//! Convention: `select` returns an index into `state.members`, or
//! `state.members.len()` to signal convergence (end the discussion).

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;

use crate::providers::{ChatMessage, ChatRequest, LlmProvider};

/// One group-chat participant.
#[derive(Debug, Clone)]
pub struct GroupMember {
    pub role_id: String,
    pub name: String,
}

/// One utterance in the shared transcript.
#[derive(Debug, Clone)]
pub struct GroupTurn {
    /// Index into `members`.
    pub speaker: usize,
    pub text: String,
}

/// Immutable-per-round view handed to selectors.
#[derive(Debug, Clone)]
pub struct GroupState {
    pub members: Vec<GroupMember>,
    pub turns: Vec<GroupTurn>,
    pub round: usize,
    pub last_speaker: Option<usize>,
    /// How many times in a row `last_speaker` has spoken.
    pub consecutive_count: usize,
}

impl GroupState {
    pub fn new(members: Vec<GroupMember>) -> Self {
        Self {
            members,
            turns: Vec::new(),
            round: 0,
            last_speaker: None,
            consecutive_count: 0,
        }
    }

    /// The discussion topic so far (task + all utterances), for relevance scoring.
    pub fn topic_text(&self) -> String {
        self.turns
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn push_turn(&mut self, speaker: usize, text: String) {
        if self.last_speaker == Some(speaker) {
            self.consecutive_count += 1;
        } else {
            self.consecutive_count = 1;
            self.last_speaker = Some(speaker);
        }
        self.turns.push(GroupTurn { speaker, text });
    }
}

/// Strategy for choosing the next speaker.
#[async_trait]
pub trait SpeakerSelector: Send + Sync {
    /// Index into members, or `members.len()` to converge/end.
    async fn select(&self, state: &GroupState) -> usize;
}

fn word_set(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Self-developed heuristic: `score = keyword-overlap(topic, member) × 2
/// + freshness (+1 for anyone else, −10 for the previous speaker)`.
///
/// Deterministic; ties go to the lowest member index.
pub struct HeuristicSelector;

impl HeuristicSelector {
    fn score(&self, state: &GroupState, idx: usize) -> i64 {
        let topic = word_set(&state.topic_text());
        let member_words = word_set(&state.members[idx].name);
        let overlap = topic.intersection(&member_words).count() as i64;
        let freshness = if state.last_speaker == Some(idx) {
            -10
        } else {
            1
        };
        overlap * 2 + freshness
    }
}

#[async_trait]
impl SpeakerSelector for HeuristicSelector {
    async fn select(&self, state: &GroupState) -> usize {
        let mut best_idx = state.members.len(); // empty team ⇒ converge
        let mut best_score = i64::MIN;
        for idx in 0..state.members.len() {
            let score = self.score(state, idx);
            if score > best_score {
                best_score = score;
                best_idx = idx;
            }
        }
        best_idx
    }
}

/// Degradation baseline when no smarter selector is available: plain rotation,
/// which never repeats the previous speaker (for teams larger than one).
pub struct RoundRobinSelector;

#[async_trait]
impl SpeakerSelector for RoundRobinSelector {
    async fn select(&self, state: &GroupState) -> usize {
        let n = state.members.len();
        if n == 0 {
            return 0;
        }
        match state.last_speaker {
            Some(last) => (last + 1) % n,
            None => 0,
        }
    }
}

/// Default LLM-judge prompt. Placeholders: `{transcript}`, `{roster}`,
/// `{converge_index}`.
pub const DEFAULT_SELECTOR_PROMPT: &str = "Discussion transcript:\n{transcript}\n\nParticipants:\n{roster}\n\nReply with JSON {{\"index\": <number>}} picking the next speaker. Use index {converge_index} to end the discussion.";

/// LLM-as-judge selector: asks a provider for `{"index": n}` JSON and falls
/// back to round-robin on malformed output. The prompt is a customizable
/// template (i18n / tuning) with `{transcript}`, `{roster}` and
/// `{converge_index}` placeholders.
pub struct LlmSelector {
    provider: Arc<dyn LlmProvider>,
    model: String,
    prompt_template: String,
}

impl LlmSelector {
    pub fn new(provider: Arc<dyn LlmProvider>, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
            prompt_template: DEFAULT_SELECTOR_PROMPT.to_string(),
        }
    }

    /// Overrides the judge prompt template.
    pub fn with_prompt_template(mut self, template: impl Into<String>) -> Self {
        self.prompt_template = template.into();
        self
    }
}

#[async_trait]
impl SpeakerSelector for LlmSelector {
    async fn select(&self, state: &GroupState) -> usize {
        let n = state.members.len();
        if n == 0 {
            return 0;
        }
        let roster = state
            .members
            .iter()
            .enumerate()
            .map(|(i, m)| format!("{i}: {}", m.name))
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = self
            .prompt_template
            .replace("{transcript}", &state.topic_text())
            .replace("{roster}", &roster)
            .replace("{converge_index}", &n.to_string());
        let request = ChatRequest {
            model: self.model.clone(),
            system_prompt: Some("You are a discussion moderator.".into()),
            messages: vec![ChatMessage::user(prompt)],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            cache_retention: Default::default(),
            cache_scope: None,
            external_session_id: None,
        };
        let Ok(resp) = self.provider.complete(&request).await else {
            return state.last_speaker.map_or(0, |last| (last + 1) % n);
        };
        let parsed = serde_json::from_str::<serde_json::Value>(resp.content.trim())
            .ok()
            .and_then(|v| v.get("index").and_then(serde_json::Value::as_u64))
            .map(|i| i as usize);
        match parsed {
            Some(idx) if idx <= n => idx,
            _ => state.last_speaker.map_or(0, |last| (last + 1) % n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::FakeLlm;

    fn members() -> Vec<GroupMember> {
        vec![
            GroupMember {
                role_id: "r1".into(),
                name: "rust coder".into(),
            },
            GroupMember {
                role_id: "r2".into(),
                name: "docs writer".into(),
            },
        ]
    }

    #[tokio::test]
    async fn heuristic_relevance_factor_prefers_topic_overlap() {
        let mut state = GroupState::new(members());
        // Topic mentions "rust" → member 0 must win despite equal freshness.
        state.push_turn(1, "let us talk about rust".into());
        let picked = HeuristicSelector.select(&state).await;
        assert_eq!(picked, 0);
    }

    #[tokio::test]
    async fn heuristic_freshness_factor_penalizes_previous_speaker() {
        let mut state = GroupState::new(members());
        // Both names appear equally in the topic → overlap ties; freshness breaks it.
        state.push_turn(0, "rust coder and docs writer agree".into());
        let picked = HeuristicSelector.select(&state).await;
        assert_eq!(picked, 1);
    }

    #[tokio::test]
    async fn heuristic_ties_go_to_lowest_index_and_first_pick_is_member_zero() {
        let state = GroupState::new(members()); // empty topic → pure freshness tie
        assert_eq!(HeuristicSelector.select(&state).await, 0);
    }

    #[tokio::test]
    async fn round_robin_rotates_and_skips_last_speaker() {
        let mut state = GroupState::new(members());
        assert_eq!(RoundRobinSelector.select(&state).await, 0);
        state.push_turn(0, "first".into());
        assert_eq!(RoundRobinSelector.select(&state).await, 1);
        state.push_turn(1, "second".into());
        assert_eq!(RoundRobinSelector.select(&state).await, 0);
    }

    #[tokio::test]
    async fn llm_selector_parses_index_json_from_fake_provider() {
        let provider = Arc::new(FakeLlm::new(
            "judge",
            vec![FakeLlm::response(r#"{"index": 1}"#)],
        ));
        let selector = LlmSelector::new(provider, "m");
        let state = GroupState::new(members());
        assert_eq!(selector.select(&state).await, 1);
    }

    #[tokio::test]
    async fn llm_selector_falls_back_to_rotation_on_garbage() {
        let provider = Arc::new(FakeLlm::new(
            "judge",
            vec![FakeLlm::response("I cannot answer that")],
        ));
        let selector = LlmSelector::new(provider, "m");
        let mut state = GroupState::new(members());
        state.push_turn(0, "hello".into());
        assert_eq!(selector.select(&state).await, 1);
    }

    #[tokio::test]
    async fn llm_selector_convergence_signal_passes_through() {
        let provider = Arc::new(FakeLlm::new(
            "judge",
            vec![FakeLlm::response(r#"{"index": 2}"#)],
        ));
        let selector = LlmSelector::new(provider, "m");
        let state = GroupState::new(members()); // len == 2 ⇒ index 2 means converge
        assert_eq!(selector.select(&state).await, 2);
    }

    #[tokio::test]
    async fn custom_prompt_template_is_used_verbatim_with_placeholders_filled() {
        let provider = Arc::new(FakeLlm::new(
            "judge",
            vec![FakeLlm::response(r#"{"index": 1}"#)],
        ));
        let selector = LlmSelector::new(provider.clone(), "m")
            .with_prompt_template("PICK|{roster}|END={converge_index}|T={transcript}");
        let mut state = GroupState::new(members());
        state.push_turn(0, "hello".into());
        assert_eq!(selector.select(&state).await, 1);
        let reqs = provider.requests.lock().unwrap();
        let sent = &reqs[0].messages[0].content;
        assert!(
            sent.starts_with("PICK|0: rust coder\n1: docs writer|END=2|T=hello"),
            "{sent}"
        );
    }
}
