//! Review gate: pluggable checkpoint between automatic reflection and the
//! prompt write-back. Every candidate produced by the evolution loop must
//! pass a [`ReviewGate`] before it reaches the version state machine —
//! a runaway reflector must never be able to corrupt the base prompt
//! unilaterally (prime-agent stability package).
//!
//! The default implementation is deliberately heuristic-only (non-empty
//! content + dedup against the most recent proposals). An LLM-judged gate
//! can be added later behind the same trait.

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;

use super::reflection::PromptCandidate;

/// A candidate prompt submitted for review before write-back.
#[derive(Debug, Clone)]
pub struct ReviewProposal {
    /// Prompt slot the candidate targets (e.g. `"system_prompt"`).
    pub plugin: String,
    pub candidate: PromptCandidate,
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
///
/// TODO(llm-review): implement an LLM-judged gate behind this trait — pass
/// the candidate plus recent rejections to a judge model and map its verdict
/// onto [`ReviewVerdict`]. Intentionally not implemented here.
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
}
