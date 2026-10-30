//! Four-track evaluation pipeline (ADR-0025 §5 Slice 4).
//!
//! Orchestrates the four evaluation tracks in sequence:
//! 1. **Track 1 — Hard baseline**: cargo test + pnpm test + typecheck + clippy.
//!    Hard gate: any failure → immediate reject.
//! 2. **Track 2 — Shadow replay**: replay golden benchmarks, compare metrics.
//!    Soft gate: degradation > threshold → soft reject (user can override).
//! 3. **Track 3 — LLM review**: `LlmReviewGate` with user_profile + diff.
//!    Advisory: produces approve/reject/defer verdict.
//! 4. **Track 4 — UX anchor probes**: E2E checks of high-confidence UI anchors.
//!    Hard gate for L2/L3; skipped for L0/L1.

use std::sync::Arc;

use async_trait::async_trait;

use crate::domain::B1Config;
use crate::evolution::snapshot_types::{
    EvaluationMetrics, Track1Result, Track2Result, Track3Result, Track4Result,
};
use crate::evolution::PreferenceService;

/// Degradation threshold for shadow replay (ADR-0025 §5.2: default 15%).
const DEFAULT_DEGRADATION_THRESHOLD_PCT: f64 = 15.0;

/// Minimum high-confidence UI anchors for Track 4 to run (ADR-0025 §5.4: < 3 → skip).
#[allow(dead_code)]
const MIN_UX_ANCHORS: u32 = 3;

/// Input for the four-track evaluator.
pub struct EvaluationInput {
    /// Path to the built artifact directory (for Track 1 test execution).
    pub artifact_dir: String,
    /// Git diff of the proposed change (for Track 3 LLM review).
    pub diff: String,
    /// Evolution tier (determines whether Track 4 runs).
    pub tier: crate::store::repos::evolution_snapshots::EvolutionTier,
    /// Database connection for preference/profile lookups.
    pub conn: rusqlite::Connection,
}

/// The overall verdict from the four-track evaluation.
#[derive(Debug, Clone, PartialEq)]
pub enum EvaluationVerdict {
    /// All tracks passed — proceed to approval and handoff.
    Approve(EvaluationMetrics),
    /// Hard gate failed (Track 1 or Track 4) — reject, do not proceed.
    HardReject {
        metrics: EvaluationMetrics,
        reason: String,
    },
    /// Soft gate failed (Track 2 degradation > threshold) — user can override.
    SoftReject {
        metrics: EvaluationMetrics,
        reason: String,
    },
    /// Track 3 LLM review deferred — hold for later.
    Defer {
        metrics: EvaluationMetrics,
        reason: String,
    },
}

/// Track 1: hard baseline test runner (ADR-0025 §5.1).
///
/// Executes `cargo test`, `pnpm test`, `pnpm typecheck`, `cargo clippy`
/// in the sandbox and collects pass/fail counts.
#[async_trait]
pub trait Track1Runner: Send + Sync {
    async fn run(&self, artifact_dir: &str) -> Track1Result;
}

/// Track 2: shadow trajectory replay runner (ADR-0025 §5.2).
///
/// Replays golden benchmark tasks against the new version and compares
/// metrics (duration, tokens, success rate) against baseline medians.
#[async_trait]
pub trait Track2Runner: Send + Sync {
    /// Returns shadow replay comparison results.
    /// `replay_count` is the configured number of benchmarks to replay.
    async fn run(&self, artifact_dir: &str, replay_count: u32) -> Track2Result;
}

/// Track 4: UX anchor E2E probe runner (ADR-0025 §5.4).
///
/// Extracts high-confidence UI preferences and runs E2E probes to verify
/// they still work in the new version.
#[async_trait]
pub trait Track4Runner: Send + Sync {
    /// Returns UX anchor check results.
    /// `min_confidence` filters which preferences to check.
    async fn run(&self, artifact_dir: &str, min_confidence: f64) -> Track4Result;
}

/// The four-track evaluator. Orchestrates all tracks in sequence.
pub struct FourTrackEvaluator {
    track1: Arc<dyn Track1Runner>,
    track2: Arc<dyn Track2Runner>,
    review_gate: Arc<dyn crate::evolution::ReviewGate>,
    track4: Option<Arc<dyn Track4Runner>>,
    degradation_threshold_pct: f64,
    shadow_replay_count: u32,
    min_confidence: f64,
}

impl FourTrackEvaluator {
    /// Create a new evaluator with the given track runners and config.
    pub fn new(
        track1: Arc<dyn Track1Runner>,
        track2: Arc<dyn Track2Runner>,
        review_gate: Arc<dyn crate::evolution::ReviewGate>,
        track4: Option<Arc<dyn Track4Runner>>,
        b1_config: &B1Config,
    ) -> Self {
        Self {
            track1,
            track2,
            review_gate,
            track4,
            degradation_threshold_pct: DEFAULT_DEGRADATION_THRESHOLD_PCT,
            shadow_replay_count: 3,
            min_confidence: b1_config.min_confidence,
        }
    }

    /// Override the degradation threshold (ADR §5.2: Compass-adjustable 5%–50%).
    pub fn with_degradation_threshold(mut self, pct: f64) -> Self {
        self.degradation_threshold_pct = pct.clamp(5.0, 50.0);
        self
    }

    /// Override the shadow replay count (ADR §5.2: default 3, L3 ≥ 5).
    pub fn with_shadow_replay_count(mut self, count: u32) -> Self {
        self.shadow_replay_count = count;
        self
    }

    /// Run the four-track evaluation.
    ///
    /// Track 1 is a hard gate: failure → immediate `HardReject`.
    /// Track 2 is a soft gate: degradation > threshold → `SoftReject`.
    /// Track 3 is advisory: `Reject` → `HardReject`, `Defer` → `Defer`.
    /// Track 4 is a hard gate for L2/L3: failure → `HardReject`.
    pub async fn evaluate(&self, input: &mut EvaluationInput) -> EvaluationVerdict {
        // Track 1: hard baseline.
        let track1 = self.track1.run(&input.artifact_dir).await;
        if !track1.passed {
            let metrics = self.assemble_metrics(track1.clone(), skip_rest(), default_t3(), None);
            return EvaluationVerdict::HardReject {
                metrics,
                reason: format!("Track 1 failed: {}", track1.summary),
            };
        }

        // Track 2: shadow replay.
        let track2 = self
            .track2
            .run(&input.artifact_dir, self.shadow_replay_count)
            .await;
        if !track2.passed && !track2.skipped {
            let metrics = self.assemble_metrics(track1.clone(), track2.clone(), default_t3(), None);
            return EvaluationVerdict::SoftReject {
                metrics,
                reason: format!(
                    "Track 2: degradation {:.1}% exceeds threshold {:.1}%",
                    track2.max_degradation_pct, self.degradation_threshold_pct
                ),
            };
        }

        // Track 3: LLM review.
        let user_profile = PreferenceService::snapshot_flattened(&input.conn).unwrap_or_default();
        let proposal = crate::evolution::ReviewProposal {
            plugin: "evolution".to_string(),
            candidate: crate::evolution::PromptCandidate {
                content: input.diff.clone(),
                diff_text: input.diff.clone(),
                parent_version: None,
            },
            user_profile,
        };
        let verdict = self.review_gate.review(&proposal).await;
        let track3 = match &verdict {
            crate::evolution::ReviewVerdict::Approve => Track3Result {
                verdict: "approve".to_string(),
                reason: String::new(),
            },
            crate::evolution::ReviewVerdict::Reject(reason) => Track3Result {
                verdict: "reject".to_string(),
                reason: reason.clone(),
            },
            crate::evolution::ReviewVerdict::Defer => Track3Result {
                verdict: "defer".to_string(),
                reason: "LLM review deferred".to_string(),
            },
        };

        if matches!(verdict, crate::evolution::ReviewVerdict::Reject(_)) {
            let metrics =
                self.assemble_metrics(track1.clone(), track2.clone(), track3.clone(), None);
            return EvaluationVerdict::HardReject {
                metrics,
                reason: format!("Track 3 rejected: {}", track3.reason),
            };
        }

        if matches!(verdict, crate::evolution::ReviewVerdict::Defer) {
            let metrics =
                self.assemble_metrics(track1.clone(), track2.clone(), track3.clone(), None);
            return EvaluationVerdict::Defer {
                metrics,
                reason: track3.reason,
            };
        }

        // Track 4: UX anchor probes (L2/L3 only).
        let track4 = if self.should_run_track4(input.tier) {
            match &self.track4 {
                Some(runner) => {
                    let result = runner.run(&input.artifact_dir, self.min_confidence).await;
                    Some(result)
                }
                None => None,
            }
        } else {
            None
        };

        if let Some(ref t4) = track4 {
            if !t4.passed && !t4.skipped {
                let metrics = self.assemble_metrics(
                    track1.clone(),
                    track2.clone(),
                    track3.clone(),
                    Some(t4.clone()),
                );
                return EvaluationVerdict::HardReject {
                    metrics,
                    reason: format!(
                        "Track 4: {} of {} UX anchors failed",
                        t4.anchors_failed, t4.anchors_checked
                    ),
                };
            }
        }

        // All tracks passed.
        let metrics = self.assemble_metrics(track1, track2, track3, track4);
        EvaluationVerdict::Approve(metrics)
    }

    /// Track 4 runs only for L2/L3 tiers (ADR-0025 §5.4).
    fn should_run_track4(
        &self,
        tier: crate::store::repos::evolution_snapshots::EvolutionTier,
    ) -> bool {
        matches!(
            tier,
            crate::store::repos::evolution_snapshots::EvolutionTier::L2
                | crate::store::repos::evolution_snapshots::EvolutionTier::L3
        )
    }

    fn assemble_metrics(
        &self,
        track1: Track1Result,
        track2: Track2Result,
        track3: Track3Result,
        track4: Option<Track4Result>,
    ) -> EvaluationMetrics {
        let mut metrics = EvaluationMetrics {
            track1,
            track2,
            track3,
            track4,
            overall_pass: false,
        };
        metrics.overall_pass = metrics.compute_overall_pass();
        metrics
    }
}

fn skip_rest() -> Track2Result {
    Track2Result {
        passed: true,
        skipped: true,
        replay_count: 0,
        max_degradation_pct: 0.0,
        metrics: vec![],
    }
}

fn default_t3() -> Track3Result {
    Track3Result {
        verdict: "defer".to_string(),
        reason: "skipped due to Track 1 failure".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evolution::snapshot_types::ShadowMetricComparison;
    use crate::evolution::DefaultReviewGate;

    struct AlwaysPassTrack1;
    #[async_trait]
    impl Track1Runner for AlwaysPassTrack1 {
        async fn run(&self, _dir: &str) -> Track1Result {
            Track1Result {
                passed: true,
                total_tests: 100,
                failures: 0,
                clippy_warnings: 0,
                typecheck_errors: 0,
                summary: "all green".into(),
            }
        }
    }

    struct AlwaysFailTrack1;
    #[async_trait]
    impl Track1Runner for AlwaysFailTrack1 {
        async fn run(&self, _dir: &str) -> Track1Result {
            Track1Result {
                passed: false,
                total_tests: 100,
                failures: 1,
                clippy_warnings: 0,
                typecheck_errors: 0,
                summary: "1 test failed".into(),
            }
        }
    }

    struct SkippedTrack2;
    #[async_trait]
    impl Track2Runner for SkippedTrack2 {
        async fn run(&self, _dir: &str, _count: u32) -> Track2Result {
            Track2Result {
                passed: true,
                skipped: true,
                replay_count: 0,
                max_degradation_pct: 0.0,
                metrics: vec![],
            }
        }
    }

    struct DegradedTrack2;
    #[async_trait]
    impl Track2Runner for DegradedTrack2 {
        async fn run(&self, _dir: &str, _count: u32) -> Track2Result {
            Track2Result {
                passed: false,
                skipped: false,
                replay_count: 3,
                max_degradation_pct: 20.0,
                metrics: vec![ShadowMetricComparison {
                    name: "duration_ms".into(),
                    baseline: 1000.0,
                    new_value: 1200.0,
                    degradation_pct: 20.0,
                }],
            }
        }
    }

    struct PassTrack4;
    #[async_trait]
    impl Track4Runner for PassTrack4 {
        async fn run(&self, _dir: &str, _conf: f64) -> Track4Result {
            Track4Result {
                passed: true,
                skipped: false,
                anchors_checked: 5,
                anchors_failed: 0,
                failed_anchor_keys: vec![],
            }
        }
    }

    struct FailTrack4;
    #[async_trait]
    impl Track4Runner for FailTrack4 {
        async fn run(&self, _dir: &str, _conf: f64) -> Track4Result {
            Track4Result {
                passed: false,
                skipped: false,
                anchors_checked: 5,
                anchors_failed: 2,
                failed_anchor_keys: vec!["kbd.save".into(), "kbd.search".into()],
            }
        }
    }

    fn fresh_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::store::migrations::run(&conn).unwrap();
        conn
    }

    fn make_evaluator(
        t1: Arc<dyn Track1Runner>,
        t2: Arc<dyn Track2Runner>,
        t4: Option<Arc<dyn Track4Runner>>,
    ) -> FourTrackEvaluator {
        let gate = Arc::new(DefaultReviewGate::new(16));
        FourTrackEvaluator::new(t1, t2, gate, t4, &B1Config::default())
    }

    #[tokio::test]
    async fn track1_fail_immediate_hard_reject() {
        let evaluator = make_evaluator(Arc::new(AlwaysFailTrack1), Arc::new(SkippedTrack2), None);
        let mut input = EvaluationInput {
            artifact_dir: "/tmp/art".into(),
            diff: "diff".into(),
            tier: crate::store::repos::evolution_snapshots::EvolutionTier::L1,
            conn: fresh_conn(),
        };
        let verdict = evaluator.evaluate(&mut input).await;
        assert!(matches!(verdict, EvaluationVerdict::HardReject { .. }));
    }

    #[tokio::test]
    async fn track2_degradation_soft_reject() {
        let evaluator = make_evaluator(Arc::new(AlwaysPassTrack1), Arc::new(DegradedTrack2), None);
        let mut input = EvaluationInput {
            artifact_dir: "/tmp/art".into(),
            diff: "diff".into(),
            tier: crate::store::repos::evolution_snapshots::EvolutionTier::L1,
            conn: fresh_conn(),
        };
        let verdict = evaluator.evaluate(&mut input).await;
        assert!(matches!(verdict, EvaluationVerdict::SoftReject { .. }));
    }

    #[tokio::test]
    async fn all_pass_approve_for_l1() {
        let evaluator = make_evaluator(Arc::new(AlwaysPassTrack1), Arc::new(SkippedTrack2), None);
        let mut input = EvaluationInput {
            artifact_dir: "/tmp/art".into(),
            diff: "valid diff content".into(),
            tier: crate::store::repos::evolution_snapshots::EvolutionTier::L1,
            conn: fresh_conn(),
        };
        let verdict = evaluator.evaluate(&mut input).await;
        match verdict {
            EvaluationVerdict::Approve(metrics) => {
                assert!(metrics.overall_pass);
                assert!(metrics.track4.is_none()); // L1 doesn't run Track 4
            }
            other => panic!("expected approve, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn track4_fail_hard_reject_for_l2() {
        let evaluator = make_evaluator(
            Arc::new(AlwaysPassTrack1),
            Arc::new(SkippedTrack2),
            Some(Arc::new(FailTrack4)),
        );
        let mut input = EvaluationInput {
            artifact_dir: "/tmp/art".into(),
            diff: "valid diff content".into(),
            tier: crate::store::repos::evolution_snapshots::EvolutionTier::L2,
            conn: fresh_conn(),
        };
        let verdict = evaluator.evaluate(&mut input).await;
        assert!(matches!(verdict, EvaluationVerdict::HardReject { .. }));
    }

    #[tokio::test]
    async fn track4_pass_approve_for_l2() {
        let evaluator = make_evaluator(
            Arc::new(AlwaysPassTrack1),
            Arc::new(SkippedTrack2),
            Some(Arc::new(PassTrack4)),
        );
        let mut input = EvaluationInput {
            artifact_dir: "/tmp/art".into(),
            diff: "valid diff content".into(),
            tier: crate::store::repos::evolution_snapshots::EvolutionTier::L2,
            conn: fresh_conn(),
        };
        let verdict = evaluator.evaluate(&mut input).await;
        match verdict {
            EvaluationVerdict::Approve(metrics) => {
                assert!(metrics.overall_pass);
                assert!(metrics.track4.is_some());
            }
            other => panic!("expected approve, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn track4_skipped_for_l0() {
        let evaluator = make_evaluator(
            Arc::new(AlwaysPassTrack1),
            Arc::new(SkippedTrack2),
            Some(Arc::new(FailTrack4)),
        );
        let mut input = EvaluationInput {
            artifact_dir: "/tmp/art".into(),
            diff: "valid diff content".into(),
            tier: crate::store::repos::evolution_snapshots::EvolutionTier::L0,
            conn: fresh_conn(),
        };
        let verdict = evaluator.evaluate(&mut input).await;
        match verdict {
            EvaluationVerdict::Approve(metrics) => {
                assert!(metrics.track4.is_none()); // L0 doesn't run Track 4
            }
            other => panic!("expected approve, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn degradation_threshold_clamped() {
        let evaluator = make_evaluator(Arc::new(AlwaysPassTrack1), Arc::new(SkippedTrack2), None)
            .with_degradation_threshold(1.0); // below 5% min
        assert_eq!(evaluator.degradation_threshold_pct, 5.0);

        let evaluator = make_evaluator(Arc::new(AlwaysPassTrack1), Arc::new(SkippedTrack2), None)
            .with_degradation_threshold(99.0); // above 50% max
        assert_eq!(evaluator.degradation_threshold_pct, 50.0);
    }
}
