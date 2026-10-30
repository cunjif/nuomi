//! Review gate: pluggable checkpoint between automatic reflection and the
//! prompt write-back. Every candidate produced by the evolution loop must
//! pass a [`ReviewGate`] before it reaches the version state machine —
//! a runaway reflector must never be able to corrupt the base prompt
//! unilaterally (prime-agent stability package).
//!
//! Two implementations: the heuristic-only [`DefaultReviewGate`] (non-empty
//! content + dedup) and [`LlmReviewGate`], which runs the cheap heuristics
//! first and then asks a judge model (SPEC D11 self-evolution; the strict
//! GEPA LLM-as-judge replication stays on the roadmap, L87).

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;

use super::reflection::PromptCandidate;
use crate::providers::{ChatRequest, LlmProvider};

/// A candidate prompt submitted for review before write-back.
#[derive(Debug, Clone)]
pub struct ReviewProposal {
    /// Prompt slot the candidate targets (e.g. `"system_prompt"`).
    pub plugin: String,
    pub candidate: PromptCandidate,
    /// User preference profile strings for context-aware review
    /// (ADR-0025 §5.3 Slice 4 Track 3).
    pub user_profile: Vec<String>,
}

/// Outcome of a review pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewVerdict {
    /// Candidate may proceed into the version state machine.
    Approve,
    /// Candidate is dropped; the reason is recorded on the evolution log.
    Reject(String),
    /// Candidate is neither approved nor dropped — hold it for a human or a
    /// later, better-informed pass.
    Defer,
}

/// Pluggable review step run before an automatic reflection → write-back.
#[async_trait]
pub trait ReviewGate: Send + Sync {
    async fn review(&self, proposal: &ReviewProposal) -> ReviewVerdict;
}

/// Heuristic-only gate: rejects empty content and proposals identical to any
/// of the last `window` accepted ones (default 16).
pub struct DefaultReviewGate {
    window: usize,
    /// Contents of the most recently accepted proposals (dedup memory).
    recent: Mutex<VecDeque<String>>,
}

impl DefaultReviewGate {
    /// `window` is how many recently accepted contents are kept for dedup;
    /// `0` disables dedup entirely.
    pub fn new(window: usize) -> Self {
        Self {
            window,
            recent: Mutex::new(VecDeque::new()),
        }
    }
}

impl Default for DefaultReviewGate {
    fn default() -> Self {
        Self::new(16)
    }
}

#[async_trait]
impl ReviewGate for DefaultReviewGate {
    async fn review(&self, proposal: &ReviewProposal) -> ReviewVerdict {
        // Heuristic 1: the proposed prompt must carry actual content.
        if proposal.candidate.content.trim().is_empty() {
            return ReviewVerdict::Reject("proposal content is empty".to_string());
        }
        // Heuristic 2: dedup — an identical proposal within the recent window
        // signals a reflection loop that stopped making progress.
        let mut recent = self
            .recent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if recent.iter().any(|c| *c == proposal.candidate.content) {
            return ReviewVerdict::Reject(
                "proposal duplicates one of the recent accepted proposals".to_string(),
            );
        }
        if self.window > 0 {
            recent.push_back(proposal.candidate.content.clone());
            while recent.len() > self.window {
                recent.pop_front();
            }
        }
        ReviewVerdict::Approve
    }
}

/// System prompt for the judge model — first-line verdict protocol.
const JUDGE_SYSTEM: &str = "\
You are the review gate of the nuomi self-evolution engine. You are given a \
candidate system prompt (with its diff) that a reflector proposes writing \
back, plus the reasons recent candidates were rejected. Judge whether the \
candidate should proceed into the version state machine. Respond with ONLY \
one first line, optionally followed by a short rationale on later lines:\n\
APPROVE — the candidate is sound and improves on its parent.\n\
REJECT: <one-line reason> — the candidate is harmful, regressive, or \
gibberish.\n\
DEFER — you cannot decide confidently with the given information.";

/// LLM-judged review gate (TODO(llm-review) cleared): cheap heuristics first
/// (empty content / duplicate of a recent accepted proposal — never worth a
/// model call), then a judge model over the candidate plus the recent
/// rejection reasons.
///
/// Failure semantics: a provider error or an unparsable reply both map to
/// [`ReviewVerdict::Defer`] — the candidate is neither approved nor dropped,
/// which is exactly "hold for a later, better-informed pass". The gate never
/// lets an infrastructure failure silently approve a write-back.
pub struct LlmReviewGate {
    heuristic: DefaultReviewGate,
    provider: Arc<dyn LlmProvider>,
    /// Ring of recent REJECT reasons fed into the next judge prompt so the
    /// judge does not keep re-litigating the same class of candidates.
    rejections: Mutex<VecDeque<String>>,
    rejections_window: usize,
}

impl LlmReviewGate {
    /// `heuristic_window` mirrors [`DefaultReviewGate::new`];
    /// `rejections_window` caps the reject-reason memory (0 disables).
    /// No `Default` impl on purpose: there is no sensible default judge
    /// provider — the caller always supplies one explicitly.
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        heuristic_window: usize,
        rejections_window: usize,
    ) -> Self {
        Self {
            heuristic: DefaultReviewGate::new(heuristic_window),
            provider,
            rejections: Mutex::new(VecDeque::new()),
            rejections_window,
        }
    }
}

#[async_trait]
impl ReviewGate for LlmReviewGate {
    async fn review(&self, proposal: &ReviewProposal) -> ReviewVerdict {
        // Stage 1: cheap heuristics — a refusal here never spends a call.
        let verdict = self.heuristic.review(proposal).await;
        if !matches!(verdict, ReviewVerdict::Approve) {
            return verdict;
        }

        // Stage 2: judge model over the candidate + recent rejection reasons.
        let user_prompt = {
            let rejections = self
                .rejections
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let recent = if rejections.is_empty() {
                "(none yet)".to_string()
            } else {
                rejections
                    .iter()
                    .map(|r| format!("- {r}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let profile = if proposal.user_profile.is_empty() {
                "(none)".to_string()
            } else {
                proposal
                    .user_profile
                    .iter()
                    .map(|p| format!("- {p}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            format!(
                "Prompt slot: {}\n\nProposed candidate:\n{}\n\nDiff against parent:\n{}\n\n\
                 User preference profile:\n{profile}\n\n\
                 Reasons recent candidates were rejected:\n{recent}",
                proposal.plugin, proposal.candidate.content, proposal.candidate.diff_text,
            )
        };
        let request = ChatRequest::simple("evolution", JUDGE_SYSTEM, &user_prompt);
        let response = match self.provider.complete(&request).await {
            Ok(response) => response,
            Err(e) => {
                // Defer, not Reject: an infrastructure failure says nothing
                // about the candidate's merit. The error is traced, not
                // surfaced in the verdict — a Defer carrying an error string
                // would read as "the judge considered and deferred", which
                // it did not.
                tracing::warn!(error = %e, "review-gate judge call failed; deferring");
                return ReviewVerdict::Defer;
            }
        };

        match parse_verdict(&response.content) {
            ReviewVerdict::Reject(reason) => {
                let mut rejections = self
                    .rejections
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                if self.rejections_window > 0 {
                    rejections.push_back(reason.clone());
                    while rejections.len() > self.rejections_window {
                        rejections.pop_front();
                    }
                }
                ReviewVerdict::Reject(reason)
            }
            other => other,
        }
    }
}

/// Maps the judge's first line onto a verdict. Unknown or missing markers
/// resolve to [`ReviewVerdict::Defer`] — the safe default that neither
/// approves nor drops the candidate.
fn parse_verdict(reply: &str) -> ReviewVerdict {
    let first = reply.lines().next().unwrap_or_default().trim();
    // to_ascii_uppercase preserves byte length, so prefix offsets carry over.
    let upper = first.to_ascii_uppercase();
    if upper.starts_with("APPROVE") {
        return ReviewVerdict::Approve;
    }
    if let Some(rest) = upper.strip_prefix("REJECT") {
        // Slice the ORIGINAL line by the same offset to keep the reason's
        // original casing.
        let original_rest = &first[first.len() - rest.len()..];
        let reason = original_rest.trim_start_matches([':', ' ']).trim();
        return if reason.is_empty() {
            ReviewVerdict::Reject("rejected by judge (no reason given)".to_string())
        } else {
            ReviewVerdict::Reject(reason.to_string())
        };
    }
    ReviewVerdict::Defer
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal(content: &str) -> ReviewProposal {
        ReviewProposal {
            plugin: "system_prompt".to_string(),
            candidate: PromptCandidate {
                content: content.to_string(),
                diff_text: "diff".to_string(),
                parent_version: None,
            },
            user_profile: vec![],
        }
    }

    #[tokio::test]
    async fn empty_content_is_rejected() {
        let gate = DefaultReviewGate::new(4);
        assert_eq!(
            gate.review(&proposal("   ")).await,
            ReviewVerdict::Reject("proposal content is empty".to_string())
        );
    }

    #[tokio::test]
    async fn duplicate_within_window_is_rejected_then_accepted_after_expiry() {
        let gate = DefaultReviewGate::new(2);
        assert_eq!(gate.review(&proposal("a")).await, ReviewVerdict::Approve);
        assert_eq!(gate.review(&proposal("b")).await, ReviewVerdict::Approve);
        // "a" is still inside the 2-entry window.
        assert!(matches!(
            gate.review(&proposal("a")).await,
            ReviewVerdict::Reject(_)
        ));
        // Two new accepts push "a" out of the window.
        assert_eq!(gate.review(&proposal("b2")).await, ReviewVerdict::Approve);
        assert_eq!(gate.review(&proposal("c")).await, ReviewVerdict::Approve);
        assert_eq!(gate.review(&proposal("a")).await, ReviewVerdict::Approve);
    }

    #[tokio::test]
    async fn rejected_duplicates_do_not_consume_window_slots() {
        let gate = DefaultReviewGate::new(1);
        assert_eq!(gate.review(&proposal("a")).await, ReviewVerdict::Approve);
        assert!(matches!(
            gate.review(&proposal("a")).await,
            ReviewVerdict::Reject(_)
        ));
        assert!(matches!(
            gate.review(&proposal("a")).await,
            ReviewVerdict::Reject(_)
        ));
        assert_eq!(gate.review(&proposal("b")).await, ReviewVerdict::Approve);
        assert_eq!(gate.review(&proposal("a")).await, ReviewVerdict::Approve);
    }

    #[tokio::test]
    async fn zero_window_disables_dedup() {
        let gate = DefaultReviewGate::new(0);
        assert_eq!(gate.review(&proposal("same")).await, ReviewVerdict::Approve);
        assert_eq!(gate.review(&proposal("same")).await, ReviewVerdict::Approve);
    }
    // -------------------------------------------------------- LlmReviewGate

    mod llm_gate {
        use super::*;
        use crate::providers::{ChatResponse, FakeLlm, LlmProvider};
        use std::sync::Arc;

        fn gate(llm: Arc<FakeLlm>) -> LlmReviewGate {
            LlmReviewGate::new(llm as Arc<dyn LlmProvider>, 4, 8)
        }

        fn judge_reply(content: &str) -> ChatResponse {
            ChatResponse {
                content: content.to_string(),
                ..ChatResponse::default()
            }
        }

        #[tokio::test]
        async fn approve_maps_through_and_prompt_carries_candidate_context() {
            let llm = Arc::new(FakeLlm::new(
                "judge",
                vec![judge_reply("APPROVE\nlooks good")],
            ));
            let g = gate(llm.clone());
            assert_eq!(
                g.review(&proposal("solid body")).await,
                ReviewVerdict::Approve
            );

            // The judge prompt carried the plugin slot, the candidate body,
            // the diff and the (empty) rejection list.
            let requests = llm.requests.lock().unwrap();
            assert_eq!(requests.len(), 1);
            let user = requests[0]
                .messages
                .last()
                .map(|m| m.content.clone())
                .unwrap_or_default();
            assert!(user.contains("system_prompt"), "prompt: {user}");
            assert!(user.contains("solid body"), "prompt: {user}");
            assert!(user.contains("diff"), "prompt: {user}");
            assert!(user.contains("(none yet)"), "prompt: {user}");
            let system = requests[0].system_prompt.clone().unwrap_or_default();
            assert!(system.contains("APPROVE"), "system: {system}");
        }

        #[tokio::test]
        async fn reject_maps_reason_and_feeds_next_prompt() {
            let llm = Arc::new(FakeLlm::new(
                "judge",
                vec![
                    judge_reply("REJECT: prompt drops safety constraints"),
                    judge_reply("APPROVE\nrevised"),
                ],
            ));
            let g = gate(llm.clone());
            assert_eq!(
                g.review(&proposal("risky body")).await,
                ReviewVerdict::Reject("prompt drops safety constraints".to_string())
            );
            assert_eq!(
                g.review(&proposal("revised body")).await,
                ReviewVerdict::Approve
            );

            // The rejection reason landed in the second judge prompt.
            let requests = llm.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            let second = requests[1]
                .messages
                .last()
                .map(|m| m.content.clone())
                .unwrap_or_default();
            assert!(
                second.contains("prompt drops safety constraints"),
                "second prompt: {second}"
            );
        }

        #[tokio::test]
        async fn defer_maps_for_both_explicit_defer_and_unparsable_reply() {
            let llm = Arc::new(FakeLlm::new(
                "judge",
                vec![
                    judge_reply("DEFER\nneed more context"),
                    judge_reply("no marker at all"),
                ],
            ));
            let g = gate(llm);
            assert_eq!(g.review(&proposal("body a")).await, ReviewVerdict::Defer);
            assert_eq!(g.review(&proposal("body b")).await, ReviewVerdict::Defer);
        }

        #[tokio::test]
        async fn provider_failure_defers_instead_of_rejecting() {
            // Empty script: the fake provider errors on exhaustion.
            let llm = Arc::new(FakeLlm::new("judge", vec![]));
            let g = gate(llm);
            assert_eq!(g.review(&proposal("body")).await, ReviewVerdict::Defer);
        }

        #[tokio::test]
        async fn heuristic_short_circuits_before_the_model_call() {
            let llm = Arc::new(FakeLlm::new("judge", vec![judge_reply("APPROVE")]));
            let g = gate(llm.clone());
            // Empty content is rejected by the built-in heuristic — the
            // judge model must never be consulted for it.
            assert!(matches!(
                g.review(&proposal("  ")).await,
                ReviewVerdict::Reject(_)
            ));
            // Duplicate of an accepted proposal likewise short-circuits.
            assert_eq!(g.review(&proposal("same")).await, ReviewVerdict::Approve);
            assert!(matches!(
                g.review(&proposal("same")).await,
                ReviewVerdict::Reject(_)
            ));
            assert_eq!(llm.requests.lock().unwrap().len(), 1);
        }

        #[test]
        fn parse_verdict_is_case_and_separator_tolerant() {
            assert_eq!(parse_verdict("approve"), ReviewVerdict::Approve);
            assert_eq!(
                parse_verdict("  APPROVE extra words"),
                ReviewVerdict::Approve
            );
            assert_eq!(
                parse_verdict("reject: lowercased reason"),
                ReviewVerdict::Reject("lowercased reason".to_string())
            );
            assert_eq!(
                parse_verdict("Reject no colon reason"),
                ReviewVerdict::Reject("no colon reason".to_string())
            );
            assert_eq!(
                parse_verdict("REJECT:   "),
                ReviewVerdict::Reject("rejected by judge (no reason given)".to_string())
            );
            assert_eq!(parse_verdict("DEFER"), ReviewVerdict::Defer);
            assert_eq!(parse_verdict(""), ReviewVerdict::Defer);
            assert_eq!(parse_verdict("sounds great!"), ReviewVerdict::Defer);
        }
    }
}
