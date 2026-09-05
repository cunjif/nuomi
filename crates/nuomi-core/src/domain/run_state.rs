//! Run lifecycle state machine — the single source of truth (AGENTS.md §4).
//!
//! Legal transitions:
//!
//! ```text
//! queued --Start--> running
//! running <--RequestApproval--> awaiting_approval
//! awaiting_approval --Resolve(_--> running   (approved or denied both resume)
//! running --Succeed|Fail|Timeout|Cancel--> succeeded|failed|timed_out|cancelled
//! running --OrphanTimeout--> interrupted     (crash recovery)
//! awaiting_approval --Cancel--> cancelled
//! interrupted --Requeue--> queued
//! ```
//!
//! Everything else is illegal. The iron rule lives with the caller: persist the
//! `state_changed` EventRecord BEFORE applying any external side effect.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// All possible run states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum RunState {
    Queued,
    Running,
    AwaitingApproval,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
    Interrupted,
}

/// Events driving the state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum RunEvent {
    Start,
    RequestApproval,
    Resolve(ApprovalOutcome),
    Succeed,
    Fail,
    Timeout,
    Cancel,
    OrphanTimeout,
    Requeue,
}

/// Outcome of an approval request; both resume the loop back to `running`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ApprovalOutcome {
    Approved,
    Denied,
}

/// The only error `transition` can produce.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid run transition: {from} --{event}-->")]
pub struct TransitionError {
    pub from: &'static str,
    pub event: String,
}

impl RunState {
    /// Total transition function: exhaustive over `(RunState, RunEvent)`.
    pub fn transition(self, ev: RunEvent) -> Result<RunState, TransitionError> {
        let next = match (self, ev) {
            (RunState::Queued, RunEvent::Start) => RunState::Running,
            (RunState::Running, RunEvent::RequestApproval) => RunState::AwaitingApproval,
            (RunState::AwaitingApproval, RunEvent::Resolve(_)) => RunState::Running,
            (RunState::Running, RunEvent::Succeed) => RunState::Succeeded,
            (RunState::Running, RunEvent::Fail) => RunState::Failed,
            (RunState::Running, RunEvent::Timeout) => RunState::TimedOut,
            (RunState::Running, RunEvent::Cancel) => RunState::Cancelled,
            (RunState::Running, RunEvent::OrphanTimeout) => RunState::Interrupted,
            (RunState::AwaitingApproval, RunEvent::Cancel) => RunState::Cancelled,
            (RunState::Interrupted, RunEvent::Requeue) => RunState::Queued,
            (from, event) => {
                return Err(TransitionError {
                    from: from.as_str(),
                    event: format!("{event:?}"),
                })
            }
        };
        Ok(next)
    }

    /// Canonical DB/IPC text form (`snake_case`, matches migration CHECKs).
    pub fn as_str(self) -> &'static str {
        match self {
            RunState::Queued => "queued",
            RunState::Running => "running",
            RunState::AwaitingApproval => "awaiting_approval",
            RunState::Succeeded => "succeeded",
            RunState::Failed => "failed",
            RunState::TimedOut => "timed_out",
            RunState::Cancelled => "cancelled",
            RunState::Interrupted => "interrupted",
        }
    }

    /// Inverse of [`RunState::as_str`].
    pub fn parse(s: &str) -> Option<RunState> {
        match s {
            "queued" => Some(RunState::Queued),
            "running" => Some(RunState::Running),
            "awaiting_approval" => Some(RunState::AwaitingApproval),
            "succeeded" => Some(RunState::Succeeded),
            "failed" => Some(RunState::Failed),
            "timed_out" => Some(RunState::TimedOut),
            "cancelled" => Some(RunState::Cancelled),
            "interrupted" => Some(RunState::Interrupted),
            _ => None,
        }
    }
}

impl std::fmt::Display for RunState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ApprovalOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalOutcome::Approved => "approved",
            ApprovalOutcome::Denied => "denied",
        }
    }
}

/// Failure modes of the generational guard ([`GenerationalRun::apply`]).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MutationError {
    /// The caller presented a generation older than the run's current one —
    /// a late event (e.g. a cancel racing a requeue). The state machine is
    /// untouched.
    #[error("stale run mutation: given generation {given}, current {current}")]
    Stale { given: u64, current: u64 },
    /// The transition itself is illegal from the current state.
    #[error(transparent)]
    Invalid(#[from] TransitionError),
}

/// A run state machine guarded by a monotonic generation counter (`run_seq`).
///
/// Equivalent of a CAS on `(state, run_seq)`: a mutation carries the
/// generation it was issued under and is applied only when that generation
/// is at least the current one; each successful transition bumps the
/// generation by one. Late events from a previous generation (a cancel or
/// requeue that raced ahead) are rejected with [`MutationError::Stale`] and
/// never touch the state.
///
/// Pure domain logic — no DB, no clock; persistence wiring lives in the
/// store layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationalRun {
    state: RunState,
    run_seq: u64,
}

impl GenerationalRun {
    pub fn new(state: RunState, run_seq: u64) -> Self {
        Self { state, run_seq }
    }

    pub fn state(&self) -> RunState {
        self.state
    }

    pub fn run_seq(&self) -> u64 {
        self.run_seq
    }

    /// Applies `event` iff `expected_seq >= self.run_seq`; on success the
    /// state advances and the generation increments. On [`MutationError`]
    /// neither state nor generation changes.
    pub fn apply(&mut self, expected_seq: u64, event: RunEvent) -> Result<RunState, MutationError> {
        if expected_seq < self.run_seq {
            return Err(MutationError::Stale {
                given: expected_seq,
                current: self.run_seq,
            });
        }
        let next = self.state.transition(event)?;
        self.state = next;
        self.run_seq = self.run_seq.wrapping_add(1);
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every legal transition: (from, event, to). AC1 requires full coverage.
    const LEGAL: &[(RunState, RunEvent, RunState)] = &[
        (RunState::Queued, RunEvent::Start, RunState::Running),
        (
            RunState::Running,
            RunEvent::RequestApproval,
            RunState::AwaitingApproval,
        ),
        (
            RunState::AwaitingApproval,
            RunEvent::Resolve(ApprovalOutcome::Approved),
            RunState::Running,
        ),
        (
            RunState::AwaitingApproval,
            RunEvent::Resolve(ApprovalOutcome::Denied),
            RunState::Running,
        ),
        (RunState::Running, RunEvent::Succeed, RunState::Succeeded),
        (RunState::Running, RunEvent::Fail, RunState::Failed),
        (RunState::Running, RunEvent::Timeout, RunState::TimedOut),
        (RunState::Running, RunEvent::Cancel, RunState::Cancelled),
        (
            RunState::Running,
            RunEvent::OrphanTimeout,
            RunState::Interrupted,
        ),
        (
            RunState::AwaitingApproval,
            RunEvent::Cancel,
            RunState::Cancelled,
        ),
        (RunState::Interrupted, RunEvent::Requeue, RunState::Queued),
    ];

    const ALL_STATES: [RunState; 8] = [
        RunState::Queued,
        RunState::Running,
        RunState::AwaitingApproval,
        RunState::Succeeded,
        RunState::Failed,
        RunState::TimedOut,
        RunState::Cancelled,
        RunState::Interrupted,
    ];

    fn all_events() -> Vec<RunEvent> {
        vec![
            RunEvent::Start,
            RunEvent::RequestApproval,
            RunEvent::Resolve(ApprovalOutcome::Approved),
            RunEvent::Resolve(ApprovalOutcome::Denied),
            RunEvent::Succeed,
            RunEvent::Fail,
            RunEvent::Timeout,
            RunEvent::Cancel,
            RunEvent::OrphanTimeout,
            RunEvent::Requeue,
        ]
    }

    #[test]
    fn table_every_legal_transition_succeeds() {
        for &(from, ev, to) in LEGAL {
            let got = from.transition(ev);
            assert_eq!(got, Ok(to), "{from:?} --{ev:?}--> expected {to:?}");
        }
    }

    #[test]
    fn table_every_illegal_transition_is_rejected_exhaustively() {
        // Exhaustive sweep over all (state, event) pairs not in LEGAL.
        for &state in &ALL_STATES {
            for ev in all_events() {
                let legal = LEGAL.iter().any(|&(f, e, _)| f == state && e == ev);
                if legal {
                    continue;
                }
                let err = state
                    .transition(ev)
                    .expect_err(&format!("expected {state:?} --{ev:?}--> to be illegal"));
                assert_eq!(err.from, state.as_str());
            }
        }
    }

    #[test]
    fn terminal_states_reject_everything() {
        for &terminal in &[
            RunState::Succeeded,
            RunState::Failed,
            RunState::TimedOut,
            RunState::Cancelled,
        ] {
            for ev in all_events() {
                assert!(terminal.transition(ev).is_err(), "{terminal:?} --{ev:?}-->");
            }
        }
    }

    #[test]
    fn orphan_recovery_roundtrip() {
        // AC6: running crash → interrupted → requeue → running again.
        let path = RunState::Queued
            .transition(RunEvent::Start)
            .and_then(|s| s.transition(RunEvent::OrphanTimeout))
            .and_then(|s| s.transition(RunEvent::Requeue))
            .and_then(|s| s.transition(RunEvent::Start));
        assert_eq!(path, Ok(RunState::Running));
    }

    #[test]
    fn approval_pause_resume_roundtrip() {
        let paused = RunState::Running
            .transition(RunEvent::RequestApproval)
            .unwrap();
        assert_eq!(paused, RunState::AwaitingApproval);
        assert_eq!(
            paused
                .transition(RunEvent::Resolve(ApprovalOutcome::Denied))
                .unwrap(),
            RunState::Running
        );
    }

    #[test]
    fn serde_state_internal_tag_camel_case_roundtrip() {
        let json = serde_json::to_string(&RunState::AwaitingApproval).unwrap();
        assert_eq!(json, r#"{"type":"awaitingApproval"}"#);
        let back: RunState = serde_json::from_str(&json).unwrap();
        assert_eq!(back, RunState::AwaitingApproval);
    }

    #[test]
    fn serde_event_and_outcome_roundtrip() {
        let json = serde_json::to_string(&RunEvent::Resolve(ApprovalOutcome::Denied)).unwrap();
        assert_eq!(json, r#"{"event":"resolve","data":{"type":"denied"}}"#);
        let back: RunEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, RunEvent::Resolve(ApprovalOutcome::Denied));

        for ev in all_events() {
            let json = serde_json::to_string(&ev).unwrap();
            assert_eq!(serde_json::from_str::<RunEvent>(&json).unwrap(), ev);
        }
    }

    #[test]
    fn as_str_parse_are_inverse_and_match_schema_check_values() {
        for &state in &ALL_STATES {
            assert_eq!(RunState::parse(state.as_str()), Some(state));
        }
        assert_eq!(RunState::parse("bogus"), None);
    }

    // --- GenerationalRun (run_seq CAS) -----------------------------------

    #[test]
    fn generational_normal_sequence_is_unaffected() {
        let mut run = GenerationalRun::new(RunState::Queued, 0);
        assert_eq!(run.apply(0, RunEvent::Start), Ok(RunState::Running));
        assert_eq!(run.run_seq(), 1);
        assert_eq!(
            run.apply(1, RunEvent::RequestApproval),
            Ok(RunState::AwaitingApproval)
        );
        assert_eq!(
            run.apply(2, RunEvent::Resolve(ApprovalOutcome::Approved)),
            Ok(RunState::Running)
        );
        assert_eq!(run.apply(3, RunEvent::Succeed), Ok(RunState::Succeeded));
        assert_eq!(run.state(), RunState::Succeeded);
        assert_eq!(run.run_seq(), 4);
    }

    #[test]
    fn generational_late_event_is_rejected_without_mutation() {
        let mut run = GenerationalRun::new(RunState::Running, 3);
        // A stale cancel issued back at generation 2 arrives after two
        // generations have passed — it must be refused verbatim.
        let err = run.apply(2, RunEvent::Cancel).unwrap_err();
        assert_eq!(
            err,
            MutationError::Stale {
                given: 2,
                current: 3
            }
        );
        assert_eq!(run.state(), RunState::Running);
        assert_eq!(run.run_seq(), 3);
        // The current generation still mutates normally afterwards.
        assert_eq!(run.apply(3, RunEvent::Succeed), Ok(RunState::Succeeded));
    }

    #[test]
    fn generational_cancel_and_requeue_only_hit_their_own_generation() {
        // Crash recovery race: orphan timeout bumps to gen 1 (interrupted),
        // a requeue lands (gen 2, queued), then the run starts again (gen 3).
        // The stale `Cancel` that was issued while the run was still on gen 0
        // must not cancel the fresh incarnation.
        let mut run = GenerationalRun::new(RunState::Running, 0);
        assert_eq!(
            run.apply(0, RunEvent::OrphanTimeout),
            Ok(RunState::Interrupted)
        );
        assert_eq!(run.apply(1, RunEvent::Requeue), Ok(RunState::Queued));
        assert!(matches!(
            run.apply(0, RunEvent::Cancel),
            Err(MutationError::Stale { .. })
        ));
        assert_eq!(run.state(), RunState::Queued);
        assert_eq!(run.apply(2, RunEvent::Start), Ok(RunState::Running));
        assert_eq!(run.run_seq(), 3);
    }

    #[test]
    fn generational_stale_check_precedes_transition_check() {
        // Even an illegal event reports staleness first when the generation
        // is behind — the guard is the outermost gate.
        let mut run = GenerationalRun::new(RunState::Succeeded, 5);
        assert!(matches!(
            run.apply(0, RunEvent::Start),
            Err(MutationError::Stale { .. })
        ));
        // At the current generation the same event surfaces as illegal.
        assert_eq!(
            run.apply(5, RunEvent::Start),
            Err(MutationError::Invalid(TransitionError {
                from: "succeeded",
                event: "Start".to_string(),
            }))
        );
        assert_eq!(run.run_seq(), 5);
    }
}
