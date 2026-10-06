//! Intent recognition — classifies user messages to steward actions.
//! (K-Steward-2, T2-5/T2-6)

use serde::{Deserialize, Serialize};

use super::snapshot::AppStateSnapshot;
use super::StewardError;

/// The 5 steward intent classes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum StewardIntent {
    /// User wants to change app configuration (providers, roles, teams, ...).
    ConfigChange {
        target: Option<String>,
        description: String,
    },
    /// User wants to trigger or discuss self-evolution.
    Evolution { instruction: String },
    /// User wants to schedule or manage tasks.
    Scheduling { description: String },
    /// User is diagnosing a problem or asking about app state.
    Diagnosis { question: String },
    /// Free-form conversation that doesn't fit other intents.
    Freeform { message: String },
}

/// The result of intent recognition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentRecognition {
    pub intent: StewardIntent,
    pub confidence: f64,
    pub rationale: String,
}

/// Trait for intent recognition implementations.
#[async_trait::async_trait]
pub trait IntentRecognizer: Send + Sync {
    async fn recognize(
        &self,
        message: &str,
        snapshot: &AppStateSnapshot,
    ) -> Result<IntentRecognition, StewardError>;
}

/// A rule-based intent recognizer used as the default when no LLM is configured.
///
/// Uses keyword matching to classify the message into one of 5 intents.
/// In production, this is replaced by `LlmIntentRecognizer` which calls a
/// lightweight LLM with JSON output constraints.
pub struct RuleIntentRecognizer;

#[async_trait::async_trait]
impl IntentRecognizer for RuleIntentRecognizer {
    async fn recognize(
        &self,
        message: &str,
        _snapshot: &AppStateSnapshot,
    ) -> Result<IntentRecognition, StewardError> {
        let lower = message.to_lowercase();
        let (intent, confidence) = if lower.contains("配置")
            || lower.contains("provider")
            || lower.contains("role")
            || lower.contains("team")
            || lower.contains("设置")
        {
            (
                StewardIntent::ConfigChange {
                    target: None,
                    description: message.to_string(),
                },
                0.8,
            )
        } else if lower.contains("进化") || lower.contains("evolution") || lower.contains("改进")
        {
            (
                StewardIntent::Evolution {
                    instruction: message.to_string(),
                },
                0.8,
            )
        } else if lower.contains("定时") || lower.contains("调度") || lower.contains("schedule")
        {
            (
                StewardIntent::Scheduling {
                    description: message.to_string(),
                },
                0.7,
            )
        } else if lower.contains("诊断") || lower.contains("问题") || lower.contains("状态") {
            (
                StewardIntent::Diagnosis {
                    question: message.to_string(),
                },
                0.7,
            )
        } else {
            (
                StewardIntent::Freeform {
                    message: message.to_string(),
                },
                0.5,
            )
        };
        Ok(IntentRecognition {
            intent,
            confidence,
            rationale: "rule-based classification".into(),
        })
    }
}

/// Confidence threshold below which the steward asks the user to clarify.
pub const CLARIFY_THRESHOLD: f64 = 0.6;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steward::snapshot::AppStateSnapshot;

    fn empty_snapshot() -> AppStateSnapshot {
        AppStateSnapshot {
            providers: vec![],
            roles: vec![],
            teams: vec![],
            agent_profiles: vec![],
            sessions: vec![],
            recent_events: vec![],
            memory_entries: vec![],
            prompt_versions: vec![],
            evolution_cycles: vec![],
        }
    }

    #[tokio::test]
    async fn config_change_intent() {
        let r = RuleIntentRecognizer;
        let snap = empty_snapshot();
        let result = r.recognize("帮我配置 provider", &snap).await.unwrap();
        assert!(matches!(result.intent, StewardIntent::ConfigChange { .. }));
        assert!(result.confidence >= CLARIFY_THRESHOLD);
    }

    #[tokio::test]
    async fn evolution_intent() {
        let r = RuleIntentRecognizer;
        let snap = empty_snapshot();
        let result = r.recognize("触发进化", &snap).await.unwrap();
        assert!(matches!(result.intent, StewardIntent::Evolution { .. }));
    }

    #[tokio::test]
    async fn freeform_intent_low_confidence() {
        let r = RuleIntentRecognizer;
        let snap = empty_snapshot();
        let result = r.recognize("你好", &snap).await.unwrap();
        assert!(matches!(result.intent, StewardIntent::Freeform { .. }));
        assert!(result.confidence < CLARIFY_THRESHOLD);
    }
}
