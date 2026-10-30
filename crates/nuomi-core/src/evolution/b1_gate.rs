//! B1 trigger gate — anti-dialog-fatigue debounce, cooldown and budget control
//! (ADR-0025 §2.1 Slice 3).
//!
//! The gate decides whether a B1 sedimentation check should run, based on:
//! - **Trigger type**: explicit user commands and complex-task completions bypass
//!   the idle debounce; idle-window triggers must wait the full debounce period.
//! - **Cooldown**: same-session proposals are forced apart by `cooldown_secs`.
//! - **Budget**: daily token consumption for silent judgments is capped.
//! - **Dynamic adaptation**: when `adaptive_enabled`, the debounce and cooldown
//!   windows are scaled by session activity level — high-frequency tool-call
//!   sessions raise thresholds, low-frequency碎片 sessions lower them.

use crate::domain::B1Config;

/// Why a trigger was rejected — for observability and UI feedback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerRejection {
    /// Last proposal was too recent; `secs_remaining` until cooldown expires.
    Cooldown { secs_remaining: u64 },
    /// Daily token budget exhausted.
    BudgetExhausted,
    /// Idle debounce window not yet elapsed; `secs_remaining` until eligible.
    Debounce { secs_remaining: u64 },
}

/// The kind of event that might trigger a B1 sedimentation check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TriggerKind {
    /// User issued an explicit affirmation or correction
    /// (e.g. "以后写 Rust 不要用 unwrap").
    ExplicitUserCommand,
    /// A complex multi-tool-call task completed and user confirmed acceptance.
    ComplexTaskCompleted,
    /// Conversation entered an idle window after sustained interaction.
    IdleWindow,
}

impl TriggerKind {
    /// Explicit triggers bypass the idle debounce (ADR §2.1.2).
    fn bypasses_debounce(&self) -> bool {
        matches!(
            self,
            TriggerKind::ExplicitUserCommand | TriggerKind::ComplexTaskCompleted
        )
    }
}

/// The decision returned by [`B1TriggerGate::check`].
#[derive(Debug, Clone, PartialEq)]
pub enum TriggerDecision {
    /// The trigger is allowed — proceed with sedimentation check.
    Allow,
    /// The trigger is rejected with a reason.
    Reject(TriggerRejection),
}

/// Session activity level for dynamic adaptation (ADR §2.1.4).
///
/// Derived from tool-call frequency in the current session:
/// - `Low`: < 5 tool calls —碎片 interactions, lower thresholds.
/// - `Normal`: 5–20 tool calls — standard thresholds.
/// - `High`: > 20 tool calls — intensive work, raise thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionActivity {
    Low,
    Normal,
    High,
}

impl SessionActivity {
    /// Multiplier applied to debounce and cooldown windows.
    /// High activity → 1.5× (raise thresholds, less interruption).
    /// Low activity → 0.7× (lower thresholds, more responsive).
    /// Normal → 1.0× (no change).
    fn multiplier(&self) -> f64 {
        match self {
            SessionActivity::Low => 0.7,
            SessionActivity::Normal => 1.0,
            SessionActivity::High => 1.5,
        }
    }

    /// Classify from tool-call count in the current session.
    pub fn from_tool_calls(count: u32) -> Self {
        if count < 5 {
            SessionActivity::Low
        } else if count <= 20 {
            SessionActivity::Normal
        } else {
            SessionActivity::High
        }
    }
}

/// Mutable state tracked by the gate across a session.
#[derive(Debug, Clone)]
pub struct B1GateState {
    /// Epoch-millis of the last accepted proposal in this session.
    last_proposal_ms: i64,
    /// Epoch-millis of the last user activity (message or tool call).
    last_activity_ms: i64,
    /// Tokens consumed today for silent sedimentation judgments.
    daily_tokens_consumed: u32,
    /// Epoch-millis of the current day boundary (for daily budget reset).
    day_boundary_ms: i64,
}

impl B1GateState {
    /// Create a new state with `now_ms` as the starting point.
    pub fn new(now_ms: i64) -> Self {
        Self {
            last_proposal_ms: 0,
            last_activity_ms: now_ms,
            daily_tokens_consumed: 0,
            day_boundary_ms: now_ms,
        }
    }

    /// Record user activity (message sent or tool call completed).
    pub fn record_activity(&mut self, now_ms: i64) {
        self.last_activity_ms = now_ms;
    }

    /// Record that a proposal was accepted at `now_ms`.
    pub fn record_proposal(&mut self, now_ms: i64) {
        self.last_proposal_ms = now_ms;
    }

    /// Record token consumption for a silent judgment.
    /// Automatically resets the daily counter if `now_ms` crosses the day boundary.
    pub fn record_tokens(&mut self, tokens: u32, now_ms: i64) {
        self.maybe_reset_day(now_ms);
        self.daily_tokens_consumed = self.daily_tokens_consumed.saturating_add(tokens);
    }

    /// Reset daily counter if 24h have passed since `day_boundary_ms`.
    fn maybe_reset_day(&mut self, now_ms: i64) {
        const MS_PER_DAY: i64 = 86_400_000;
        if now_ms - self.day_boundary_ms >= MS_PER_DAY {
            self.daily_tokens_consumed = 0;
            self.day_boundary_ms = now_ms;
        }
    }
}

/// The B1 trigger gate. Stateless config + mutable per-session state.
pub struct B1TriggerGate {
    config: B1Config,
}

impl B1TriggerGate {
    /// Create a gate with the given config.
    pub fn new(config: B1Config) -> Self {
        Self { config }
    }

    /// Create a gate with default config.
    pub fn with_defaults() -> Self {
        Self::new(B1Config::default())
    }

    /// Returns the effective debounce window in seconds, after dynamic adaptation.
    pub fn effective_debounce_secs(&self, activity: SessionActivity) -> u64 {
        if !self.config.adaptive_enabled {
            return self.config.idle_debounce_secs;
        }
        let scaled = self.config.idle_debounce_secs as f64 * activity.multiplier();
        scaled.round() as u64
    }

    /// Returns the effective cooldown window in seconds, after dynamic adaptation.
    pub fn effective_cooldown_secs(&self, activity: SessionActivity) -> u64 {
        if !self.config.adaptive_enabled {
            return self.config.cooldown_secs;
        }
        let scaled = self.config.cooldown_secs as f64 * activity.multiplier();
        scaled.round() as u64
    }

    /// Check whether a trigger should fire at `now_ms`.
    ///
    /// Evaluation order: day-reset → budget → cooldown → debounce.
    /// Explicit triggers bypass debounce but not cooldown or budget.
    pub fn check(
        &self,
        kind: TriggerKind,
        state: &mut B1GateState,
        activity: SessionActivity,
        now_ms: i64,
    ) -> TriggerDecision {
        // 0. Reset daily counter if 24h have passed.
        state.maybe_reset_day(now_ms);

        // 1. Budget check (applies to all trigger kinds).
        if state.daily_tokens_consumed >= self.config.daily_token_budget {
            return TriggerDecision::Reject(TriggerRejection::BudgetExhausted);
        }

        // 2. Cooldown check (applies to all trigger kinds).
        if state.last_proposal_ms > 0 {
            let cooldown_ms = self.effective_cooldown_secs(activity) * 1000;
            let elapsed = now_ms - state.last_proposal_ms;
            if elapsed < cooldown_ms as i64 {
                let remaining = (cooldown_ms as i64 - elapsed) / 1000;
                return TriggerDecision::Reject(TriggerRejection::Cooldown {
                    secs_remaining: remaining.max(0) as u64,
                });
            }
        }

        // 3. Debounce check (skipped for explicit triggers).
        if !kind.bypasses_debounce() {
            let debounce_ms = self.effective_debounce_secs(activity) * 1000;
            let idle_elapsed = now_ms - state.last_activity_ms;
            if idle_elapsed < debounce_ms as i64 {
                let remaining = (debounce_ms as i64 - idle_elapsed) / 1000;
                return TriggerDecision::Reject(TriggerRejection::Debounce {
                    secs_remaining: remaining.max(0) as u64,
                });
            }
        }

        TriggerDecision::Allow
    }

    /// Returns a reference to the config.
    pub fn config(&self) -> &B1Config {
        &self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_700_000_000_000; // fixed epoch for deterministic tests

    fn fresh_state() -> B1GateState {
        B1GateState::new(NOW)
    }

    #[test]
    fn explicit_command_bypasses_debounce() {
        let gate = B1TriggerGate::with_defaults();
        let mut state = fresh_state(); // last_activity = NOW, so debounce not elapsed
        let decision = gate.check(
            TriggerKind::ExplicitUserCommand,
            &mut state,
            SessionActivity::Normal,
            NOW,
        );
        assert_eq!(decision, TriggerDecision::Allow);
    }

    #[test]
    fn complex_task_bypasses_debounce() {
        let gate = B1TriggerGate::with_defaults();
        let mut state = fresh_state();
        let decision = gate.check(
            TriggerKind::ComplexTaskCompleted,
            &mut state,
            SessionActivity::Normal,
            NOW,
        );
        assert_eq!(decision, TriggerDecision::Allow);
    }

    #[test]
    fn idle_window_rejected_within_debounce() {
        let gate = B1TriggerGate::with_defaults();
        let mut state = fresh_state();
        state.record_activity(NOW); // activity at NOW
                                    // Check 30s later — debounce is 60s, so rejected.
        let decision = gate.check(
            TriggerKind::IdleWindow,
            &mut state,
            SessionActivity::Normal,
            NOW + 30_000,
        );
        match decision {
            TriggerDecision::Reject(TriggerRejection::Debounce { secs_remaining }) => {
                assert!(secs_remaining > 0 && secs_remaining <= 30);
            }
            other => panic!("expected debounce rejection, got {other:?}"),
        }
    }

    #[test]
    fn idle_window_allowed_after_debounce() {
        let gate = B1TriggerGate::with_defaults();
        let mut state = fresh_state();
        state.record_activity(NOW);
        // Check 61s later — debounce is 60s, so allowed.
        let decision = gate.check(
            TriggerKind::IdleWindow,
            &mut state,
            SessionActivity::Normal,
            NOW + 61_000,
        );
        assert_eq!(decision, TriggerDecision::Allow);
    }

    #[test]
    fn cooldown_rejects_second_proposal() {
        let gate = B1TriggerGate::with_defaults();
        let mut state = fresh_state();
        // First proposal accepted.
        let d1 = gate.check(
            TriggerKind::ExplicitUserCommand,
            &mut state,
            SessionActivity::Normal,
            NOW,
        );
        assert_eq!(d1, TriggerDecision::Allow);
        state.record_proposal(NOW);

        // Second proposal 5min later — cooldown is 10min, so rejected.
        let d2 = gate.check(
            TriggerKind::ExplicitUserCommand,
            &mut state,
            SessionActivity::Normal,
            NOW + 300_000,
        );
        match d2 {
            TriggerDecision::Reject(TriggerRejection::Cooldown { secs_remaining }) => {
                assert!(secs_remaining > 0 && secs_remaining <= 300);
            }
            other => panic!("expected cooldown rejection, got {other:?}"),
        }
    }

    #[test]
    fn cooldown_allows_after_expiry() {
        let gate = B1TriggerGate::with_defaults();
        let mut state = fresh_state();
        state.record_proposal(NOW);
        // 11min later — cooldown is 10min, so allowed.
        let decision = gate.check(
            TriggerKind::ExplicitUserCommand,
            &mut state,
            SessionActivity::Normal,
            NOW + 660_000,
        );
        assert_eq!(decision, TriggerDecision::Allow);
    }

    #[test]
    fn budget_exhausted_rejects_all() {
        let config = B1Config {
            daily_token_budget: 100,
            ..B1Config::default()
        };
        let gate = B1TriggerGate::new(config);
        let mut state = fresh_state();
        state.record_tokens(100, NOW);
        let decision = gate.check(
            TriggerKind::ExplicitUserCommand,
            &mut state,
            SessionActivity::Normal,
            NOW,
        );
        assert_eq!(
            decision,
            TriggerDecision::Reject(TriggerRejection::BudgetExhausted)
        );
    }

    #[test]
    fn daily_budget_resets_after_24h() {
        let config = B1Config {
            daily_token_budget: 100,
            ..B1Config::default()
        };
        let gate = B1TriggerGate::new(config);
        let mut state = fresh_state();
        state.record_tokens(100, NOW);
        // 25h later — budget should reset.
        let decision = gate.check(
            TriggerKind::ExplicitUserCommand,
            &mut state,
            SessionActivity::Normal,
            NOW + 25 * 3_600_000,
        );
        assert_eq!(decision, TriggerDecision::Allow);
    }

    #[test]
    fn high_activity_raises_debounce() {
        let gate = B1TriggerGate::with_defaults();
        let mut state = fresh_state();
        state.record_activity(NOW);
        // High activity: debounce = 60 * 1.5 = 90s.
        // At 61s — would pass normal debounce but not high-activity.
        let decision = gate.check(
            TriggerKind::IdleWindow,
            &mut state,
            SessionActivity::High,
            NOW + 61_000,
        );
        assert!(matches!(
            decision,
            TriggerDecision::Reject(TriggerRejection::Debounce { .. })
        ));
    }

    #[test]
    fn low_activity_lowers_debounce() {
        let gate = B1TriggerGate::with_defaults();
        let mut state = fresh_state();
        state.record_activity(NOW);
        // Low activity: debounce = 60 * 0.7 = 42s.
        // At 43s — passes low-activity debounce.
        let decision = gate.check(
            TriggerKind::IdleWindow,
            &mut state,
            SessionActivity::Low,
            NOW + 43_000,
        );
        assert_eq!(decision, TriggerDecision::Allow);
    }

    #[test]
    fn adaptive_disabled_uses_raw_config() {
        let config = B1Config {
            adaptive_enabled: false,
            ..B1Config::default()
        };
        let gate = B1TriggerGate::new(config);
        assert_eq!(gate.effective_debounce_secs(SessionActivity::High), 60);
        assert_eq!(gate.effective_debounce_secs(SessionActivity::Low), 60);
        assert_eq!(gate.effective_cooldown_secs(SessionActivity::High), 600);
    }

    #[test]
    fn session_activity_classification() {
        assert_eq!(SessionActivity::from_tool_calls(0), SessionActivity::Low);
        assert_eq!(SessionActivity::from_tool_calls(4), SessionActivity::Low);
        assert_eq!(SessionActivity::from_tool_calls(5), SessionActivity::Normal);
        assert_eq!(
            SessionActivity::from_tool_calls(20),
            SessionActivity::Normal
        );
        assert_eq!(SessionActivity::from_tool_calls(21), SessionActivity::High);
    }
}
