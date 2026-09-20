//! GEPA-inspired reflective prompt evolution: trajectories + user profile +
//! long-term memories + current prompt → a new prompt candidate with a diff
//! (AC12, AC14). Deliberately *not* a paper reproduction — only the
//! reflect-then-propose loop is borrowed.

use std::sync::Arc;

use serde_json::Value;

use crate::domain::RefineConfig;
use crate::providers::{ChatRequest, LlmProvider};

use super::journal::{audit, EvolutionJournal, JournalKind};
use super::trajectory::TrajectorySummary;
use super::EvolutionError;

/// A proposed next system prompt, ready for versioned storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptCandidate {
    pub content: String,
    /// Human/LLM-readable diff against the parent version.
    pub diff_text: String,
    /// Assigned later by [`super::PromptVersionManager::propose_candidate`].
    pub parent_version: Option<i64>,
}

/// Everything reflection considers — profile and cross-session memory are
/// first-class inputs, not just the current session (AC14).
#[derive(Debug, Clone, Default)]
pub struct ReflectionInput {
    pub trajectories: Vec<TrajectorySummary>,
    pub user_profile: Vec<String>,
    pub long_term_memories: Vec<String>,
    pub current_prompt: String,
}

const REFLECTION_SYSTEM: &str = "\
You are the self-evolution engine of the nuomi agent harness. Given session \
trajectories, the user profile, long-term memories and the current system \
prompt, propose an improved system prompt. Respond with ONLY a JSON object: \
{\"content\": \"<new prompt>\", \"diff\": \"<concise diff summary>\"}";

/// Drives one reflection round through an [`LlmProvider`].
pub struct Reflector {
    provider: Arc<dyn LlmProvider>,
    /// Optional Harness Journal: reflection triggers become audit entries.
    journal: Option<Arc<EvolutionJournal>>,
    /// Reserved refine configuration (GEPA-style parameters). Stored but
    /// not yet consumed — the full `/refine` pipeline is a future milestone.
    refine_config: Option<RefineConfig>,
}

impl Reflector {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            journal: None,
            refine_config: None,
        }
    }

    /// Attaches the Harness Journal for audit instrumentation.
    pub fn with_journal(mut self, journal: Arc<EvolutionJournal>) -> Self {
        self.journal = Some(journal);
        self
    }

    /// Attaches refine configuration (GEPA-style parameters). The config is
    /// stored for downstream consumption by the future `/refine` pipeline;
    /// the current `reflect` method does not yet use it.
    pub fn with_refine_config(mut self, config: RefineConfig) -> Self {
        self.refine_config = Some(config);
        self
    }

    /// Returns the attached refine configuration, if any.
    pub fn refine_config(&self) -> Option<&RefineConfig> {
        self.refine_config.as_ref()
    }

    pub async fn reflect(
        &self,
        input: &ReflectionInput,
    ) -> Result<PromptCandidate, EvolutionError> {
        if input.trajectories.is_empty() {
            return Err(EvolutionError::NoTrajectories);
        }
        audit(
            &self.journal,
            "system_prompt",
            JournalKind::ReflectionTriggered,
            "reflector",
            format!("reflection over {} trajectories", input.trajectories.len()),
            input
                .trajectories
                .iter()
                .map(|t| t.session_id.clone())
                .collect(),
            serde_json::json!({ "trajectory_count": input.trajectories.len() }),
        );
        let user_prompt = build_prompt(input);
        let request = ChatRequest::simple("evolution", REFLECTION_SYSTEM, &user_prompt);
        let response = self
            .provider
            .complete(&request)
            .await
            .map_err(|e| EvolutionError::Provider(e.to_string()))?;
        parse_reflection(&response.content)
    }
}

fn build_prompt(input: &ReflectionInput) -> String {
    let mut sections = Vec::new();
    let trajectories: Vec<Value> = input
        .trajectories
        .iter()
        .map(|t| {
            serde_json::json!({
                "session_id": t.session_id,
                "turns": t.turns,
                "tool_calls": t.tool_calls,
                "outcomes": t.outcomes,
            })
        })
        .collect();
    sections.push(format!(
        "## Trajectories\n{}",
        serde_json::to_string_pretty(&trajectories).unwrap_or_default()
    ));

    if !input.user_profile.is_empty() {
        sections.push(format!(
            "## User profile\n{}",
            input
                .user_profile
                .iter()
                .map(|l| format!("- {l}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if !input.long_term_memories.is_empty() {
        sections.push(format!(
            "## Long-term memories\n{}",
            input
                .long_term_memories
                .iter()
                .map(|l| format!("- {l}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    sections.push(format!(
        "## Current system prompt\n{}",
        input.current_prompt
    ));
    sections.join("\n\n")
}

/// Extracts the outermost JSON object from an LLM reply (tolerates code
/// fences and prose around it).
fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end < start {
        return None;
    }
    Some(&text[start..=end])
}

fn parse_reflection(text: &str) -> Result<PromptCandidate, EvolutionError> {
    let raw = extract_json_object(text).ok_or_else(|| {
        EvolutionError::InvalidReflectionOutput("no JSON object found".to_string())
    })?;
    let value: Value = serde_json::from_str(raw)
        .map_err(|e| EvolutionError::InvalidReflectionOutput(e.to_string()))?;
    let content = value
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            EvolutionError::InvalidReflectionOutput("missing string field 'content'".to_string())
        })?;
    let diff_text = value
        .get("diff")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Ok(PromptCandidate {
        content: content.to_string(),
        diff_text: diff_text.to_string(),
        parent_version: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::MemoryService;
    use crate::providers::FakeLlm;
    use crate::store::migrations;

    fn summary(id: &str, turns: usize, tool_calls: usize, outcomes: &[&str]) -> TrajectorySummary {
        TrajectorySummary {
            session_id: id.into(),
            turns,
            tool_calls,
            outcomes: outcomes.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn json_response(content: &str, diff: &str) -> crate::providers::ChatResponse {
        crate::providers::FakeLlm::response(
            &serde_json::json!({
                "content": content,
                "diff": diff,
            })
            .to_string(),
        )
    }

    #[tokio::test]
    async fn parses_valid_json_reply_into_candidate() {
        let llm = Arc::new(FakeLlm::new(
            "fake",
            vec![json_response("better prompt", "+tool guidance")],
        ));
        let reflector = Reflector::new(llm.clone());
        let input = ReflectionInput {
            trajectories: vec![summary("s1", 2, 1, &["a", "b"])],
            ..Default::default()
        };
        let candidate = reflector.reflect(&input).await.unwrap();
        assert_eq!(candidate.content, "better prompt");
        assert_eq!(candidate.diff_text, "+tool guidance");
        assert_eq!(candidate.parent_version, None);
    }

    #[tokio::test]
    async fn tolerates_code_fences_around_json() {
        let fenced = "```json\n{\"content\": \"p\", \"diff\": \"d\"}\n```";
        let llm = Arc::new(FakeLlm::new("fake", vec![FakeLlm::response(fenced)]));
        let candidate = Reflector::new(llm)
            .reflect(&ReflectionInput {
                trajectories: vec![summary("s1", 1, 0, &["x"])],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(candidate.content, "p");
    }

    #[tokio::test]
    async fn invalid_json_is_an_error() {
        let llm = Arc::new(FakeLlm::new(
            "fake",
            vec![FakeLlm::response("no json here")],
        ));
        let err = Reflector::new(llm)
            .reflect(&ReflectionInput {
                trajectories: vec![summary("s1", 1, 0, &["x"])],
                ..Default::default()
            })
            .await
            .unwrap_err();
        assert!(matches!(err, EvolutionError::InvalidReflectionOutput(_)));
    }

    #[tokio::test]
    async fn empty_trajectories_short_circuits_without_llm_call() {
        let llm = Arc::new(FakeLlm::new("fake", vec![]));
        let err = Reflector::new(llm)
            .reflect(&ReflectionInput::default())
            .await
            .unwrap_err();
        assert!(matches!(err, EvolutionError::NoTrajectories));
    }

    /// AC14: user profile AND cross-session memory both reach the LLM input.
    #[tokio::test]
    async fn profile_and_cross_session_memory_are_part_of_reflection_input() {
        let dir = tempfile::tempdir().unwrap();
        let db_path: Arc<str> = Arc::from(dir.path().join("evo.db").to_string_lossy().to_string());
        // Ensure migrations exist before MemoryService writes.
        {
            let conn = rusqlite::Connection::open(db_path.as_ref()).unwrap();
            migrations::run(&conn).unwrap();
        }
        let mem = MemoryService::new(db_path.clone());
        mem.remember(
            "the user prefers concise answers".into(),
            None,
            vec!["style".into()],
            "preference",
            true, // user_profile
        )
        .await
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        mem.remember(
            "team uses pnpm workspaces".into(),
            None,
            vec!["workflow".into()],
            "note",
            false,
        )
        .await
        .unwrap();

        let profile: Vec<String> = mem
            .user_profile()
            .await
            .unwrap()
            .into_iter()
            .map(|m| m.content)
            .collect();
        let memories: Vec<String> = mem
            .recall(None, None, 10)
            .await
            .unwrap()
            .into_iter()
            .filter(|m| !m.user_profile)
            .map(|m| m.content)
            .collect();

        let llm = Arc::new(FakeLlm::new("fake", vec![json_response("evolved", "diff")]));
        let candidate = Reflector::new(llm.clone())
            .reflect(&ReflectionInput {
                trajectories: vec![
                    summary("s1", 2, 1, &["fix bug", "done"]),
                    summary("s2", 1, 0, &["greet", "hi"]),
                ],
                user_profile: profile,
                long_term_memories: memories,
                current_prompt: "You are nuomi.".into(),
            })
            .await
            .unwrap();
        assert_eq!(candidate.content, "evolved");

        let requests = llm.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        let sent = format!(
            "{} {}",
            requests[0].system_prompt.as_deref().unwrap_or_default(),
            requests[0]
                .messages
                .iter()
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        );
        assert!(
            sent.contains("the user prefers concise answers"),
            "profile missing from LLM input"
        );
        assert!(
            sent.contains("team uses pnpm workspaces"),
            "cross-session memory missing from LLM input"
        );
        assert!(sent.contains("s1"), "trajectory ids missing from LLM input");
    }
}
