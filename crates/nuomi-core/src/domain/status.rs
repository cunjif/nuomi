//! Derived run display status — a pure read-time computation, never persisted.
//!
//! "Persisted-facts minimalism" (agent-orchestrator): the DB keeps only hard
//! facts (machine state, heartbeat timestamp, pending approvals, orphan
//! detection outcome). Anything softer — "is this run stalled?", "did the
//! worker die?" — is derived on read by [`derive_status`]. There is exactly
//! one place where display status is computed, so no second parallel state
//! convention can appear.
//!
//! Decision rules, in strict priority order (first match wins):
//!
//! 1. **Terminal** — the persisted machine state is terminal (`succeeded`,
//!    `failed`, `timed_out`, `cancelled`) or `interrupted`; those map 1:1 and
//!    override everything else.
//! 2. **Normal non-running** — `queued` maps to Queued; `awaiting_approval`
//!    (or an approval-pending fact on any non-terminal state) maps to
//!    AwaitingApproval.
//! 3. **Heartbeat window** — only for `running`:
//!    - the orphan detector already fired, or the heartbeat is at least
//!      `orphan_after_ms` old, or missing entirely → Interrupted (crash
//!      assumed, requeue candidate);
//!    - otherwise a heartbeat at least `stall_after_ms` old → Stalled.
//! 4. **Normal** — otherwise Running.
//!
//! `orphan_after_ms` must be `>= stall_after_ms`; a missing heartbeat on a
//! running run is treated as worst case (Interrupted) rather than Stalled.

use super::run_state::RunState;

/// The persisted facts `derive_status` reads from. Every field already exists
/// as a domain/store fact — nothing here is new state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunFacts {
    /// Persisted machine state (`runs.status`).
    pub state: RunState,
    /// A sensitive-action approval is pending for this run.
    pub approval_pending: bool,
    /// Unix-ms of the last worker heartbeat (`None`: no heartbeat recorded).
    pub last_heartbeat_at: Option<i64>,
    /// The orphan detector already declared this run crashed.
    pub orphaned: bool,
    /// Heartbeat age (ms) beyond which a running run shows as Stalled.
    pub stall_after_ms: i64,
    /// Heartbeat age (ms) beyond which a running run escalates to
    /// Interrupted (must be >= `stall_after_ms`).
    pub orphan_after_ms: i64,
}

/// Display-only status; computed on read, never written back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DerivedStatus {
    Queued,
    Running,
    AwaitingApproval,
    /// Running but no recent heartbeat — still inside the orphan window.
    Stalled,
    /// Crashed / heartbeat window exceeded — requeue candidate.
    Interrupted,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

impl DerivedStatus {
    /// Canonical text form (snake_case, matching `RunState::as_str` style).
    pub fn as_str(self) -> &'static str {
        match self {
            DerivedStatus::Queued => "queued",
            DerivedStatus::Running => "running",
            DerivedStatus::AwaitingApproval => "awaiting_approval",
            DerivedStatus::Stalled => "stalled",
            DerivedStatus::Interrupted => "interrupted",
            DerivedStatus::Succeeded => "succeeded",
            DerivedStatus::Failed => "failed",
            DerivedStatus::TimedOut => "timed_out",
            DerivedStatus::Cancelled => "cancelled",
        }
    }
}

/// Computes the display status from persisted facts at time `now_ms`.
pub fn derive_status(facts: &RunFacts, now_ms: i64) -> DerivedStatus {
    // 1. Terminal / recovery states override everything.
    match facts.state {
        RunState::Succeeded => return DerivedStatus::Succeeded,
        RunState::Failed => return DerivedStatus::Failed,
        RunState::TimedOut => return DerivedStatus::TimedOut,
        RunState::Cancelled => return DerivedStatus::Cancelled,
        RunState::Interrupted => return DerivedStatus::Interrupted,
        RunState::Queued => return DerivedStatus::Queued,
        RunState::AwaitingApproval => return DerivedStatus::AwaitingApproval,
        RunState::Running => {}
    }

    // 2. Approval wait outranks heartbeat staleness.
    if facts.approval_pending {
        return DerivedStatus::AwaitingApproval;
    }

    // 3. Heartbeat window on a running run.
    let age = facts.last_heartbeat_at.map_or(i64::MAX, |at| now_ms - at);
    if facts.orphaned || age >= facts.orphan_after_ms {
        return DerivedStatus::Interrupted;
    }
    if age >= facts.stall_after_ms {
        return DerivedStatus::Stalled;
    }

    // 4. Healthy.
    DerivedStatus::Running
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fresh, healthy running run: heartbeat ticks every second.
    fn healthy() -> RunFacts {
        RunFacts {
            state: RunState::Running,
            approval_pending: false,
            last_heartbeat_at: Some(1_000),
            orphaned: false,
            stall_after_ms: 30_000,
            orphan_after_ms: 120_000,
        }
    }

    #[test]
    fn table_derive_status_priority_rules() {
        let cases: Vec<(&str, RunFacts, i64, DerivedStatus)> = vec![
            // --- 1. persisted non-running states map 1:1 -------------------
            (
                "queued",
                RunFacts {
                    state: RunState::Queued,
                    ..healthy()
                },
                2_000,
                DerivedStatus::Queued,
            ),
            (
                "awaiting_approval_state",
                RunFacts {
                    state: RunState::AwaitingApproval,
                    ..healthy()
                },
                2_000,
                DerivedStatus::AwaitingApproval,
            ),
            (
                "interrupted_state",
                RunFacts {
                    state: RunState::Interrupted,
                    ..healthy()
                },
                999_999,
                DerivedStatus::Interrupted,
            ),
            (
                "succeeded_beats_stale_heartbeat",
                RunFacts {
                    state: RunState::Succeeded,
                    last_heartbeat_at: Some(0),
                    ..healthy()
                },
                10_000_000,
                DerivedStatus::Succeeded,
            ),
            (
                "failed",
                RunFacts {
                    state: RunState::Failed,
                    ..healthy()
                },
                2_000,
                DerivedStatus::Failed,
            ),
            (
                "timed_out",
                RunFacts {
                    state: RunState::TimedOut,
                    ..healthy()
                },
                2_000,
                DerivedStatus::TimedOut,
            ),
            (
                "cancelled_beats_pending_approval",
                RunFacts {
                    state: RunState::Cancelled,
                    approval_pending: true,
                    ..healthy()
                },
                2_000,
                DerivedStatus::Cancelled,
            ),
            // --- 2. approval wait outranks heartbeat window ----------------
            (
                "approval_pending_shows_awaiting",
                RunFacts {
                    approval_pending: true,
                    ..healthy()
                },
                1_000_000,
                DerivedStatus::AwaitingApproval,
            ),
            // --- 3a. heartbeat missing / orphaned => Interrupted ------------
            (
                "no_heartbeat_is_interrupted",
                RunFacts {
                    last_heartbeat_at: None,
                    ..healthy()
                },
                2_000,
                DerivedStatus::Interrupted,
            ),
            (
                "orphan_flag_is_interrupted",
                RunFacts {
                    orphaned: true,
                    ..healthy()
                },
                2_000,
                DerivedStatus::Interrupted,
            ),
            (
                "heartbeat_beyond_orphan_window",
                RunFacts { ..healthy() },
                1_120_000,
                DerivedStatus::Interrupted,
            ),
            // --- 3b. stall window => Stalled --------------------------------
            (
                "heartbeat_beyond_stall_window",
                RunFacts { ..healthy() },
                31_000,
                DerivedStatus::Stalled,
            ),
            (
                "heartbeat_exactly_at_stall_boundary",
                RunFacts { ..healthy() },
                31_000,
                DerivedStatus::Stalled,
            ),
            // --- 4. healthy --------------------------------------------------
            (
                "fresh_heartbeat_is_running",
                RunFacts { ..healthy() },
                2_000,
                DerivedStatus::Running,
            ),
            (
                "heartbeat_just_inside_stall_window",
                RunFacts { ..healthy() },
                31_000 - 1,
                DerivedStatus::Running,
            ),
        ];
        for (name, facts, now, expected) in cases {
            assert_eq!(derive_status(&facts, now), expected, "case: {name}");
        }
    }

    #[test]
    fn stall_and_orphan_boundaries_are_window_inclusive() {
        // age == orphan_after_ms escalates to Interrupted, not Stalled.
        let facts = RunFacts {
            stall_after_ms: 10,
            orphan_after_ms: 20,
            last_heartbeat_at: Some(0),
            ..healthy()
        };
        assert_eq!(derive_status(&facts, 19), DerivedStatus::Stalled);
        assert_eq!(derive_status(&facts, 20), DerivedStatus::Interrupted);
        assert_eq!(derive_status(&facts, 9), DerivedStatus::Running);
    }

    #[test]
    fn derived_status_as_str_is_snake_case() {
        assert_eq!(
            DerivedStatus::AwaitingApproval.as_str(),
            "awaiting_approval"
        );
        assert_eq!(DerivedStatus::Stalled.as_str(), "stalled");
    }
}
