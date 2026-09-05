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

use std::collections::HashMap;

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

// --- Self-landing pipeline (parallel-code) --------------------------------
//
// An independent, composable machine for how a delegated sub-run lands its
// own work. It runs BESIDE the run lifecycle above — it never reuses or
// reinterprets RunState, whose semantics stay untouched.
//
// Mapping from parallel-code's five landing states (`mcp/types.ts:113`):
// - `landing_pending`   → `Pending` (verification not yet reported)
// - (implicit in-flight verification) → `Verifying` — split out so the
//   structured `record_verification` gate has its own phase
// - (implicit in-flight merge) → `Merging` — merging is the side-effectful
//   step, so it gets its own phase; callers persist the phase transition
//   BEFORE merging, mirroring the run state machine's iron rule
// - `landing_escalated` + `pending_review` → `Escalated` — both mean "a
//   human/coordinator must take over"; nuomi has a single approvals inbox,
//   so the distinction carries no domain weight
// - `failed` + `cleanup_failed` → `Failed` — a cleanup failure is still a
//   terminal failure at the domain level; cleanup retry policy belongs to
//   the executor layer
// - landed/reviewed → `Landed`
//
// Trade-off: five states collapse into six phases here — we split the two
// implicit in-flight stages (verification, merge) because nuomi's event
// sourcing needs a persisted phase per side effect, and we collapse the
// escalated/reviewed pair because there is exactly one escalation surface.
//
// Legal transitions:
//
// ```text
// pending --RecordVerification(all passed)--> verifying
// pending --RecordVerification(any failed)--> escalated
// verifying --BeginMerge--> merging
// merging --MarkLanded--> landed
// pending|verifying|merging --Escalate--> escalated   (explicit escalation)
// pending|verifying|merging --Fail--> failed
// ```
//
// `escalated`, `failed` and `landed` are terminal; a retry is a fresh
// `LandingTracker::begin` for a new attempt id.

/// Phases of the self-landing pipeline for a delegated sub-run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum LandingPhase {
    Pending,
    Verifying,
    Merging,
    Escalated,
    Failed,
    Landed,
}

/// Events driving the landing pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum LandingEvent {
    RecordVerification { all_passed: bool },
    BeginMerge,
    MarkLanded,
    Escalate,
    Fail,
}

/// The only error [`LandingPhase::transition`] can produce.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid landing transition: {from} --{event}-->")]
pub struct LandingTransitionError {
    pub from: &'static str,
    pub event: String,
}

impl LandingPhase {
    /// Total transition function: exhaustive over `(LandingPhase, LandingEvent)`.
    pub fn transition(self, ev: LandingEvent) -> Result<LandingPhase, LandingTransitionError> {
        let next = match (self, ev) {
            (LandingPhase::Pending, LandingEvent::RecordVerification { all_passed: true }) => {
                LandingPhase::Verifying
            }
            (LandingPhase::Pending, LandingEvent::RecordVerification { all_passed: false }) => {
                LandingPhase::Escalated
            }
            (LandingPhase::Verifying, LandingEvent::BeginMerge) => LandingPhase::Merging,
            (LandingPhase::Merging, LandingEvent::MarkLanded) => LandingPhase::Landed,
            (
                LandingPhase::Pending | LandingPhase::Verifying | LandingPhase::Merging,
                LandingEvent::Escalate,
            ) => LandingPhase::Escalated,
            (
                LandingPhase::Pending | LandingPhase::Verifying | LandingPhase::Merging,
                LandingEvent::Fail,
            ) => LandingPhase::Failed,
            (from, event) => {
                return Err(LandingTransitionError {
                    from: from.as_str(),
                    event: format!("{event:?}"),
                })
            }
        };
        Ok(next)
    }

    /// Canonical DB/IPC text form (`snake_case`).
    pub fn as_str(self) -> &'static str {
        match self {
            LandingPhase::Pending => "pending",
            LandingPhase::Verifying => "verifying",
            LandingPhase::Merging => "merging",
            LandingPhase::Escalated => "escalated",
            LandingPhase::Failed => "failed",
            LandingPhase::Landed => "landed",
        }
    }

    /// Inverse of [`LandingPhase::as_str`].
    pub fn parse(s: &str) -> Option<LandingPhase> {
        match s {
            "pending" => Some(LandingPhase::Pending),
            "verifying" => Some(LandingPhase::Verifying),
            "merging" => Some(LandingPhase::Merging),
            "escalated" => Some(LandingPhase::Escalated),
            "failed" => Some(LandingPhase::Failed),
            "landed" => Some(LandingPhase::Landed),
            _ => None,
        }
    }
}

impl std::fmt::Display for LandingPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Errors produced by [`LandingTracker`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LandingError {
    #[error("unknown landing run: {0}")]
    UnknownRun(String),

    #[error("landing run already tracked: {0}")]
    AlreadyTracked(String),

    #[error(transparent)]
    Invalid(#[from] LandingTransitionError),
}

/// A tracked run's current landing record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LandingRecord {
    pub phase: LandingPhase,
    /// Last reported verification checks (name, passed) — parallel-code's
    /// "self-reported but structured" verification.
    pub checks: Vec<(String, bool)>,
    /// Reason recorded by `escalate` / `fail`.
    pub reason: Option<String>,
}

/// Tracks the landing pipeline of many runs in memory.
///
/// Pure domain logic — no DB, no clock; persistence wiring lives in the
/// store layer. The iron rule lives with the caller: persist the phase
/// change BEFORE any external side effect (e.g. before actually merging).
#[derive(Debug, Default, Clone)]
pub struct LandingTracker {
    runs: HashMap<String, LandingRecord>,
}

impl LandingTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `run_id` at [`LandingPhase::Pending`].
    pub fn begin(&mut self, run_id: impl Into<String>) -> Result<LandingPhase, LandingError> {
        let run_id = run_id.into();
        if self.runs.contains_key(&run_id) {
            return Err(LandingError::AlreadyTracked(run_id));
        }
        self.runs.insert(
            run_id.clone(),
            LandingRecord {
                phase: LandingPhase::Pending,
                checks: Vec::new(),
                reason: None,
            },
        );
        Ok(LandingPhase::Pending)
    }

    /// Records structured verification checks. All passed → `Verifying`
    /// (cleared to merge); any failed → `Escalated`. Empty checks count as
    /// vacuously passed.
    pub fn record_verification(
        &mut self,
        run_id: &str,
        checks: Vec<(String, bool)>,
    ) -> Result<LandingPhase, LandingError> {
        let all_passed = checks.iter().all(|&(_, passed)| passed);
        let phase = self.apply(run_id, LandingEvent::RecordVerification { all_passed })?;
        if let Some(record) = self.runs.get_mut(run_id) {
            record.checks = checks;
        }
        Ok(phase)
    }

    /// `Verifying → Merging`; callers persist this, then perform the merge.
    pub fn begin_merge(&mut self, run_id: &str) -> Result<LandingPhase, LandingError> {
        self.apply(run_id, LandingEvent::BeginMerge)
    }

    /// `Merging → Landed` once the merge actually succeeded.
    pub fn mark_landed(&mut self, run_id: &str) -> Result<LandingPhase, LandingError> {
        self.apply(run_id, LandingEvent::MarkLanded)
    }

    /// Explicit escalation to a human/coordinator from any active phase.
    pub fn escalate(
        &mut self,
        run_id: &str,
        reason: impl Into<String>,
    ) -> Result<LandingPhase, LandingError> {
        let phase = self.apply(run_id, LandingEvent::Escalate)?;
        if let Some(record) = self.runs.get_mut(run_id) {
            record.reason = Some(reason.into());
        }
        Ok(phase)
    }

    /// Terminal failure from any active phase.
    pub fn fail(
        &mut self,
        run_id: &str,
        reason: impl Into<String>,
    ) -> Result<LandingPhase, LandingError> {
        let phase = self.apply(run_id, LandingEvent::Fail)?;
        if let Some(record) = self.runs.get_mut(run_id) {
            record.reason = Some(reason.into());
        }
        Ok(phase)
    }

    /// Current phase of `run_id`.
    pub fn phase(&self, run_id: &str) -> Result<LandingPhase, LandingError> {
        self.runs
            .get(run_id)
            .map(|r| r.phase)
            .ok_or_else(|| LandingError::UnknownRun(run_id.to_string()))
    }

    /// Full record (phase, checks, reason) of `run_id`, if tracked.
    pub fn get(&self, run_id: &str) -> Option<&LandingRecord> {
        self.runs.get(run_id)
    }

    fn apply(&mut self, run_id: &str, ev: LandingEvent) -> Result<LandingPhase, LandingError> {
        let record = self
            .runs
            .get_mut(run_id)
            .ok_or_else(|| LandingError::UnknownRun(run_id.to_string()))?;
        let next = record.phase.transition(ev)?;
        record.phase = next;
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

    // --- LandingPhase (self-landing pipeline) ------------------------------

    /// Every legal landing transition: (from, event, to).
    const LANDING_LEGAL: &[(LandingPhase, LandingEvent, LandingPhase)] = &[
        (
            LandingPhase::Pending,
            LandingEvent::RecordVerification { all_passed: true },
            LandingPhase::Verifying,
        ),
        (
            LandingPhase::Pending,
            LandingEvent::RecordVerification { all_passed: false },
            LandingPhase::Escalated,
        ),
        (
            LandingPhase::Verifying,
            LandingEvent::BeginMerge,
            LandingPhase::Merging,
        ),
        (
            LandingPhase::Merging,
            LandingEvent::MarkLanded,
            LandingPhase::Landed,
        ),
        (
            LandingPhase::Pending,
            LandingEvent::Escalate,
            LandingPhase::Escalated,
        ),
        (
            LandingPhase::Verifying,
            LandingEvent::Escalate,
            LandingPhase::Escalated,
        ),
        (
            LandingPhase::Merging,
            LandingEvent::Escalate,
            LandingPhase::Escalated,
        ),
        (
            LandingPhase::Pending,
            LandingEvent::Fail,
            LandingPhase::Failed,
        ),
        (
            LandingPhase::Verifying,
            LandingEvent::Fail,
            LandingPhase::Failed,
        ),
        (
            LandingPhase::Merging,
            LandingEvent::Fail,
            LandingPhase::Failed,
        ),
    ];

    const LANDING_ALL_STATES: [LandingPhase; 6] = [
        LandingPhase::Pending,
        LandingPhase::Verifying,
        LandingPhase::Merging,
        LandingPhase::Escalated,
        LandingPhase::Failed,
        LandingPhase::Landed,
    ];

    fn all_landing_events() -> Vec<LandingEvent> {
        vec![
            LandingEvent::RecordVerification { all_passed: true },
            LandingEvent::RecordVerification { all_passed: false },
            LandingEvent::BeginMerge,
            LandingEvent::MarkLanded,
            LandingEvent::Escalate,
            LandingEvent::Fail,
        ]
    }

    #[test]
    fn landing_every_legal_transition_succeeds() {
        for &(from, ev, to) in LANDING_LEGAL {
            let got = from.transition(ev);
            assert_eq!(got, Ok(to), "{from:?} --{ev:?}--> expected {to:?}");
        }
    }

    #[test]
    fn landing_every_illegal_transition_is_rejected_exhaustively() {
        // Exhaustive sweep over all (phase, event) pairs not in LANDING_LEGAL.
        for &phase in &LANDING_ALL_STATES {
            for ev in all_landing_events() {
                let legal = LANDING_LEGAL.iter().any(|&(f, e, _)| f == phase && e == ev);
                if legal {
                    continue;
                }
                let err = phase
                    .transition(ev)
                    .expect_err(&format!("expected {phase:?} --{ev:?}--> to be illegal"));
                assert_eq!(err.from, phase.as_str());
            }
        }
    }

    #[test]
    fn landing_terminal_states_reject_everything() {
        for &terminal in &[
            LandingPhase::Escalated,
            LandingPhase::Failed,
            LandingPhase::Landed,
        ] {
            for ev in all_landing_events() {
                assert!(terminal.transition(ev).is_err(), "{terminal:?} --{ev:?}-->");
            }
        }
    }

    #[test]
    fn landing_serde_phase_roundtrip() {
        for &phase in &LANDING_ALL_STATES {
            let json = serde_json::to_string(&phase).unwrap();
            assert_eq!(serde_json::from_str::<LandingPhase>(&json).unwrap(), phase);
        }
    }

    #[test]
    fn landing_tracker_happy_path_lands() {
        let mut t = LandingTracker::new();
        assert_eq!(t.begin("run-1"), Ok(LandingPhase::Pending));
        let phase = t
            .record_verification(
                "run-1",
                vec![("tests".to_string(), true), ("lint".to_string(), true)],
            )
            .unwrap();
        assert_eq!(phase, LandingPhase::Verifying);
        assert_eq!(t.begin_merge("run-1").unwrap(), LandingPhase::Merging);
        assert_eq!(t.mark_landed("run-1").unwrap(), LandingPhase::Landed);
        assert_eq!(t.phase("run-1").unwrap(), LandingPhase::Landed);
    }

    #[test]
    fn landing_tracker_failed_check_escalates_with_checks_recorded() {
        let mut t = LandingTracker::new();
        t.begin("run-1").unwrap();
        let phase = t
            .record_verification(
                "run-1",
                vec![("tests".to_string(), true), ("build".to_string(), false)],
            )
            .unwrap();
        assert_eq!(phase, LandingPhase::Escalated);
        let record = t.get("run-1").unwrap();
        assert_eq!(record.checks.len(), 2);
        // Escalated is terminal: every further transition is rejected.
        assert!(t.record_verification("run-1", vec![]).is_err());
        assert!(t.begin_merge("run-1").is_err());
        assert!(t.escalate("run-1", "again").is_err());
        assert!(t.fail("run-1", "again").is_err());
    }

    #[test]
    fn landing_tracker_escalate_and_fail_carry_reasons() {
        let mut t = LandingTracker::new();
        t.begin("r1").unwrap();
        assert_eq!(
            t.escalate("r1", "needs human merge").unwrap(),
            LandingPhase::Escalated
        );
        assert_eq!(
            t.get("r1").unwrap().reason.as_deref(),
            Some("needs human merge")
        );
        t.begin("r2").unwrap();
        assert_eq!(
            t.fail("r2", "merge conflict").unwrap(),
            LandingPhase::Failed
        );
        assert_eq!(
            t.get("r2").unwrap().reason.as_deref(),
            Some("merge conflict")
        );
    }

    #[test]
    fn landing_tracker_rejects_unknown_and_duplicate_runs() {
        let mut t = LandingTracker::new();
        assert!(matches!(t.phase("nope"), Err(LandingError::UnknownRun(_))));
        assert!(matches!(
            t.record_verification("nope", vec![]),
            Err(LandingError::UnknownRun(_))
        ));
        assert!(matches!(
            t.begin_merge("nope"),
            Err(LandingError::UnknownRun(_))
        ));
        t.begin("r").unwrap();
        assert!(matches!(t.begin("r"), Err(LandingError::AlreadyTracked(_))));
    }

    #[test]
    fn landing_tracker_invalid_mid_sequence_transitions() {
        let mut t = LandingTracker::new();
        t.begin("r").unwrap();
        // Landed before verifying/merging is illegal.
        assert!(t.mark_landed("r").is_err());
        assert!(t.begin_merge("r").is_err());
        // Verification must come first; direct merge is illegal even after a
        // verify-less flow is attempted.
        assert!(matches!(
            t.begin_merge("r"),
            Err(LandingError::Invalid(LandingTransitionError {
                from: "pending",
                ..
            }))
        ));
    }
}
