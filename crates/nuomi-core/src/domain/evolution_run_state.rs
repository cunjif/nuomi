//! B2-L3 evolution run lifecycle state machine (ADR-0025 §6.5 Slice 5).
//!
//! Distinct from [`super::run_state::RunState`] (task execution loop) and
//! [`super::steward_enums::CyclePhase`] (B1 knowledge production). This state
//! machine models the binary delivery flow: compile → evaluate → stage → replace.
//!
//! ```text
//! queued ─▶ compiling ─▶ evaluating ─▶ awaiting_approval ─▶ staging ─▶ replacing ─▶ succeeded
//!    │         │            │                  │                     │
//!    └─────────┴────────────┴──────────────────┴─────────────────────┴────▶ failed / cancelled
//! ```
//!
//! **Iron rule** (AGENTS.md §4): every transition must persist the
//! `evolution.state_changed` event BEFORE any external side effect.

use serde::{Deserialize, Serialize};

/// B2-L3 evolution run lifecycle states (ADR-0025 §6.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvolutionRunState {
    Queued,
    Compiling,
    Evaluating,
    AwaitingApproval,
    Staging,
    Replacing,
    Succeeded,
    Failed,
    Cancelled,
}

impl EvolutionRunState {
    pub fn as_str(&self) -> &'static str {
        match self {
            EvolutionRunState::Queued => "queued",
            EvolutionRunState::Compiling => "compiling",
            EvolutionRunState::Evaluating => "evaluating",
            EvolutionRunState::AwaitingApproval => "awaiting_approval",
            EvolutionRunState::Staging => "staging",
            EvolutionRunState::Replacing => "replacing",
            EvolutionRunState::Succeeded => "succeeded",
            EvolutionRunState::Failed => "failed",
            EvolutionRunState::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "compiling" => EvolutionRunState::Compiling,
            "evaluating" => EvolutionRunState::Evaluating,
            "awaiting_approval" => EvolutionRunState::AwaitingApproval,
            "staging" => EvolutionRunState::Staging,
            "replacing" => EvolutionRunState::Replacing,
            "succeeded" => EvolutionRunState::Succeeded,
            "failed" => EvolutionRunState::Failed,
            "cancelled" => EvolutionRunState::Cancelled,
            _ => EvolutionRunState::Queued,
        }
    }

    /// Returns `true` if this state is terminal (no further transitions).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            EvolutionRunState::Succeeded | EvolutionRunState::Failed | EvolutionRunState::Cancelled
        )
    }

    /// Returns `true` if this state is active (non-terminal).
    pub fn is_active(&self) -> bool {
        !self.is_terminal()
    }
}

impl std::fmt::Display for EvolutionRunState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Validates whether a transition from `from` to `to` is allowed (ADR-0025 §6.5).
///
/// Returns `Ok(())` if the transition is valid, or `Err(reason)` if not.
pub fn transition(from: EvolutionRunState, to: EvolutionRunState) -> Result<(), String> {
    use EvolutionRunState::*;
    // Self-transitions are never valid (state must change).
    if from == to {
        return Err(format!("self-transition on {from}"));
    }
    let allowed = match from {
        Queued => matches!(to, Compiling | Failed | Cancelled),
        Compiling => matches!(to, Evaluating | Failed | Cancelled),
        Evaluating => matches!(to, AwaitingApproval | Failed | Cancelled),
        AwaitingApproval => matches!(to, Staging | Failed | Cancelled),
        Staging => matches!(to, Replacing | Failed | Cancelled),
        Replacing => matches!(to, Succeeded | Failed | Cancelled),
        Succeeded | Failed | Cancelled => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(format!("invalid transition: {from} → {to}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_transitions() {
        use EvolutionRunState::*;
        let path = [
            Queued,
            Compiling,
            Evaluating,
            AwaitingApproval,
            Staging,
            Replacing,
            Succeeded,
        ];
        for w in path.windows(2) {
            assert!(transition(w[0], w[1]).is_ok(), "{:?} → {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn any_active_state_can_fail() {
        use EvolutionRunState::*;
        for from in [
            Queued,
            Compiling,
            Evaluating,
            AwaitingApproval,
            Staging,
            Replacing,
        ] {
            assert!(transition(from, Failed).is_ok(), "{from} → Failed");
        }
    }

    #[test]
    fn any_active_state_can_cancel() {
        use EvolutionRunState::*;
        for from in [
            Queued,
            Compiling,
            Evaluating,
            AwaitingApproval,
            Staging,
            Replacing,
        ] {
            assert!(transition(from, Cancelled).is_ok(), "{from} → Cancelled");
        }
    }

    #[test]
    fn terminal_states_reject_all_transitions() {
        use EvolutionRunState::*;
        for terminal in [Succeeded, Failed, Cancelled] {
            for to in [
                Queued,
                Compiling,
                Evaluating,
                AwaitingApproval,
                Staging,
                Replacing,
                Succeeded,
                Failed,
                Cancelled,
            ] {
                if terminal != to {
                    assert!(transition(terminal, to).is_err(), "{terminal} → {to}");
                }
            }
        }
    }

    #[test]
    fn no_skip_ahead() {
        use EvolutionRunState::*;
        // Cannot skip compiling.
        assert!(transition(Queued, Evaluating).is_err());
        assert!(transition(Queued, AwaitingApproval).is_err());
        assert!(transition(Queued, Succeeded).is_err());
        // Cannot skip evaluating.
        assert!(transition(Compiling, AwaitingApproval).is_err());
        // Cannot skip staging.
        assert!(transition(AwaitingApproval, Replacing).is_err());
    }

    #[test]
    fn no_backward() {
        use EvolutionRunState::*;
        assert!(transition(Compiling, Queued).is_err());
        assert!(transition(Evaluating, Compiling).is_err());
        assert!(transition(Succeeded, Replacing).is_err());
    }

    #[test]
    fn self_transition_rejected() {
        use EvolutionRunState::*;
        for s in [
            Queued,
            Compiling,
            Evaluating,
            AwaitingApproval,
            Staging,
            Replacing,
            Succeeded,
            Failed,
            Cancelled,
        ] {
            assert!(transition(s, s).is_err(), "self-transition on {s}");
        }
    }

    #[test]
    fn parse_roundtrip() {
        for s in [
            EvolutionRunState::Queued,
            EvolutionRunState::Compiling,
            EvolutionRunState::Evaluating,
            EvolutionRunState::AwaitingApproval,
            EvolutionRunState::Staging,
            EvolutionRunState::Replacing,
            EvolutionRunState::Succeeded,
            EvolutionRunState::Failed,
            EvolutionRunState::Cancelled,
        ] {
            assert_eq!(EvolutionRunState::parse(s.as_str()), s);
        }
    }

    #[test]
    fn is_terminal_and_active() {
        assert!(EvolutionRunState::Succeeded.is_terminal());
        assert!(EvolutionRunState::Failed.is_terminal());
        assert!(EvolutionRunState::Cancelled.is_terminal());
        assert!(!EvolutionRunState::Queued.is_terminal());
        assert!(!EvolutionRunState::Replacing.is_terminal());
        assert!(EvolutionRunState::Queued.is_active());
        assert!(!EvolutionRunState::Succeeded.is_active());
    }
}
