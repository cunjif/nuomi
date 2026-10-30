//! Strong-type contracts for evolution snapshots (ADR-0025 §3-§5 Slice 4).
//!
//! `ProfileSummary` and `EvaluationMetrics` are the single Rust definitions
//! serialised to JSON in the `evolution_snapshots` table columns
//! `profile_summary` and `metrics`. Keeping them as typed structs prevents
//! unstructured JSON from breaking UI parsing (ADR-0025 AC33, O4).

use serde::{Deserialize, Serialize};

/// Snapshot of the user preference profile at the time an evolution snapshot
/// is created (ADR-0025 §3, AC33).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfileSummary {
    /// Number of confirmed preferences at snapshot time.
    pub preference_count: u32,
    /// Average confidence across confirmed preferences (0.0–1.0).
    pub avg_confidence: f64,
    /// Distribution of preferences by category (category → count).
    pub category_counts: Vec<CategoryCount>,
    /// Flattened preference strings (for reflection input replay).
    pub flattened: Vec<String>,
}

/// A category → count pair for [`ProfileSummary::category_counts`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CategoryCount {
    pub category: String,
    pub count: u32,
}

/// Four-track evaluation metrics (ADR-0025 §5 Slice 4).
///
/// Populated after the four-track evaluator runs. Each track produces a
/// pass/fail plus structured details for the Compass UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvaluationMetrics {
    /// Track 1: hard baseline (cargo test + pnpm test + typecheck + clippy).
    pub track1: Track1Result,
    /// Track 2: shadow trajectory replay with quantitative thresholds.
    pub track2: Track2Result,
    /// Track 3: LLM review gate verdict.
    pub track3: Track3Result,
    /// Track 4: UX anchor E2E probes (L2/L3 only; None for L0/L1).
    pub track4: Option<Track4Result>,
    /// Overall verdict — `true` only if all applicable tracks pass.
    pub overall_pass: bool,
}

/// Track 1: objective test suite hard baseline (ADR-0025 §5.1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Track1Result {
    pub passed: bool,
    /// Number of tests run.
    pub total_tests: u32,
    /// Number of failures.
    pub failures: u32,
    /// Clippy warnings (must be 0 to pass).
    pub clippy_warnings: u32,
    /// Typecheck errors (must be 0 to pass).
    pub typecheck_errors: u32,
    /// Summary line for UI display.
    pub summary: String,
}

/// Track 2: shadow trajectory replay (ADR-0025 §5.2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Track2Result {
    pub passed: bool,
    /// `true` if skipped due to insufficient golden benchmarks (cold start).
    pub skipped: bool,
    /// Number of golden benchmarks replayed.
    pub replay_count: u32,
    /// Maximum degradation across all metrics (percentage, 0.0 = no degradation).
    /// ADR §5.2: > 15% triggers soft reject.
    pub max_degradation_pct: f64,
    /// Per-metric comparison results.
    pub metrics: Vec<ShadowMetricComparison>,
}

/// A single shadow replay metric comparison (ADR-0025 §5.2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShadowMetricComparison {
    /// Metric name: `duration_ms`, `token_count`, `success_rate`.
    pub name: String,
    /// Baseline median (recent 10 same-type tasks).
    pub baseline: f64,
    /// New version's measured value.
    pub new_value: f64,
    /// Degradation percentage: `(new - baseline) / baseline * 100`.
    /// Positive = slower/worse, negative = faster/better.
    pub degradation_pct: f64,
}

/// Track 3: LLM review gate verdict (ADR-0025 §5.3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Track3Result {
    /// `"approve"`, `"reject"`, or `"defer"`.
    pub verdict: String,
    /// Reviewer's reason text.
    pub reason: String,
}

/// Track 4: UX anchor E2E probes (ADR-0025 §5.4).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Track4Result {
    pub passed: bool,
    /// `true` if skipped due to insufficient high-confidence UI anchors (< 3).
    pub skipped: bool,
    /// Number of UI anchors checked.
    pub anchors_checked: u32,
    /// Number of anchors that failed.
    pub anchors_failed: u32,
    /// Details of failed anchors for UI display.
    pub failed_anchor_keys: Vec<String>,
}

impl EvaluationMetrics {
    /// Returns `true` if all applicable tracks pass.
    /// Track 4 is only required when present (L2/L3).
    pub fn compute_overall_pass(&self) -> bool {
        let t4_pass = self.track4.as_ref().is_none_or(|t| t.passed);
        self.track1.passed && self.track2.passed && t4_pass
    }
}

impl ProfileSummary {
    /// Create a summary with zero preferences (for root snapshots).
    pub fn empty() -> Self {
        Self {
            preference_count: 0,
            avg_confidence: 0.0,
            category_counts: vec![],
            flattened: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_summary_empty() {
        let s = ProfileSummary::empty();
        assert_eq!(s.preference_count, 0);
        assert!(s.flattened.is_empty());
    }

    #[test]
    fn evaluation_metrics_overall_pass_all_green() {
        let m = EvaluationMetrics {
            track1: Track1Result {
                passed: true,
                total_tests: 100,
                failures: 0,
                clippy_warnings: 0,
                typecheck_errors: 0,
                summary: "all green".into(),
            },
            track2: Track2Result {
                passed: true,
                skipped: false,
                replay_count: 3,
                max_degradation_pct: 5.0,
                metrics: vec![],
            },
            track3: Track3Result {
                verdict: "approve".into(),
                reason: "looks good".into(),
            },
            track4: Some(Track4Result {
                passed: true,
                skipped: false,
                anchors_checked: 5,
                anchors_failed: 0,
                failed_anchor_keys: vec![],
            }),
            overall_pass: true,
        };
        assert!(m.compute_overall_pass());
    }

    #[test]
    fn evaluation_metrics_overall_fail_track1() {
        let m = EvaluationMetrics {
            track1: Track1Result {
                passed: false,
                total_tests: 100,
                failures: 1,
                clippy_warnings: 0,
                typecheck_errors: 0,
                summary: "1 failure".into(),
            },
            track2: Track2Result {
                passed: true,
                skipped: false,
                replay_count: 3,
                max_degradation_pct: 0.0,
                metrics: vec![],
            },
            track3: Track3Result {
                verdict: "approve".into(),
                reason: "".into(),
            },
            track4: None,
            overall_pass: false,
        };
        assert!(!m.compute_overall_pass());
    }

    #[test]
    fn evaluation_metrics_overall_pass_no_track4() {
        let m = EvaluationMetrics {
            track1: Track1Result {
                passed: true,
                total_tests: 10,
                failures: 0,
                clippy_warnings: 0,
                typecheck_errors: 0,
                summary: "ok".into(),
            },
            track2: Track2Result {
                passed: true,
                skipped: true,
                replay_count: 0,
                max_degradation_pct: 0.0,
                metrics: vec![],
            },
            track3: Track3Result {
                verdict: "approve".into(),
                reason: "".into(),
            },
            track4: None,
            overall_pass: true,
        };
        assert!(m.compute_overall_pass());
    }

    #[test]
    fn evaluation_metrics_serde_roundtrip() {
        let m = EvaluationMetrics {
            track1: Track1Result {
                passed: true,
                total_tests: 50,
                failures: 0,
                clippy_warnings: 0,
                typecheck_errors: 0,
                summary: "all pass".into(),
            },
            track2: Track2Result {
                passed: true,
                skipped: false,
                replay_count: 3,
                max_degradation_pct: 2.5,
                metrics: vec![ShadowMetricComparison {
                    name: "duration_ms".into(),
                    baseline: 1000.0,
                    new_value: 1025.0,
                    degradation_pct: 2.5,
                }],
            },
            track3: Track3Result {
                verdict: "approve".into(),
                reason: "good".into(),
            },
            track4: Some(Track4Result {
                passed: true,
                skipped: false,
                anchors_checked: 3,
                anchors_failed: 0,
                failed_anchor_keys: vec![],
            }),
            overall_pass: true,
        };
        let json = serde_json::to_string(&m).unwrap();
        let decoded: EvaluationMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(m, decoded);
    }
}
