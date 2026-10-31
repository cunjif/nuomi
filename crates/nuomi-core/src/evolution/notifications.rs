//! Evolution lifecycle telemetry notifications (ADR-0025 §6.8).
//!
//! Reuses the M-BOT1 `OutboundSink` abstraction to push structured card
//! notifications at three key evolution lifecycle nodes:
//!
//! - [`EvolutionNotification::Succeeded`] — version jump, layer, preference
//!   tags, shadow replay performance comparison.
//! - [`EvolutionNotification::Failed`] — failure reason (unit test failures
//!   or performance degradation), with retry/abort guidance.
//! - [`EvolutionNotification::CrashRollback`] — high-priority alert with
//!   crash count, Panic stack trace, restored version, Safe Mode guidance.
//!
//! The notifier is a thin formatting + fan-out layer: it builds `(title,
//! body)` strings and dispatches to every enabled sink via
//! [`OutboundSink::send`]. HTTP failures are logged and never propagated
//! — telemetry is best-effort.

use std::sync::Arc;

use crate::integrations::{OutboundError, OutboundSink};

/// Evolution layer that triggered the notification (ADR-0025 §2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvolutionLayer {
    /// L0 — SystemPrompt overlay.
    L0,
    /// L1 — Tool/hook plugin.
    L1,
    /// L2 — CLI agent / provider config.
    L2,
    /// L3 — Full binary replacement (B2 heavy kernel).
    L3,
}

impl EvolutionLayer {
    fn as_str(&self) -> &'static str {
        match self {
            Self::L0 => "L0",
            Self::L1 => "L1",
            Self::L2 => "L2",
            Self::L3 => "L3",
        }
    }
}

/// One evolution lifecycle notification to be dispatched to all sinks.
#[derive(Debug, Clone)]
pub enum EvolutionNotification {
    /// `evolution.succeeded` — a B2 evolution was approved and applied.
    Succeeded {
        from_version: String,
        to_version: String,
        layer: EvolutionLayer,
        preference_tags: Vec<String>,
        shadow_improvement_pct: Option<f64>,
    },
    /// `evolution.failed` — evaluation or build failed.
    Failed {
        snapshot_id: String,
        reason: FailureReason,
        details: String,
    },
    /// `evolution.crash_rollback` — crash loop triggered auto-rollback.
    CrashRollback {
        crash_count: u32,
        panic_stack: String,
        restored_version: String,
    },
}

/// Categorises why an evolution failed (ADR-0025 §6.8).
#[derive(Debug, Clone)]
pub enum FailureReason {
    /// Unit tests did not pass.
    UnitTestFailed,
    /// Shadow replay showed performance degradation.
    PerformanceDegraded,
    /// Build or compilation error.
    BuildFailed,
    /// User rejected the proposal.
    UserRejected,
    /// Other failure.
    Other,
}

impl FailureReason {
    fn as_str(&self) -> &'static str {
        match self {
            Self::UnitTestFailed => "unit_test_failed",
            Self::PerformanceDegraded => "performance_degraded",
            Self::BuildFailed => "build_failed",
            Self::UserRejected => "user_rejected",
            Self::Other => "other",
        }
    }
}

impl EvolutionNotification {
    /// Formats the notification into `(title, body)` for `OutboundSink::send`.
    pub fn format(&self) -> (String, String) {
        match self {
            Self::Succeeded {
                from_version,
                to_version,
                layer,
                preference_tags,
                shadow_improvement_pct,
            } => {
                let title = format!(
                    "evolution.succeeded | {} → {} | {}",
                    from_version,
                    to_version,
                    layer.as_str()
                );
                let mut body = String::new();
                if !preference_tags.is_empty() {
                    body.push_str(&format!("偏好标签: {}\n", preference_tags.join(", ")));
                }
                if let Some(pct) = shadow_improvement_pct {
                    body.push_str(&format!("影子回放性能变化: {pct:+.1}%"));
                } else {
                    body.push_str("影子回放性能变化: N/A");
                }
                (title, body)
            }
            Self::Failed {
                snapshot_id,
                reason,
                details,
            } => {
                let title = format!("evolution.failed | {} | {}", reason.as_str(), snapshot_id);
                let body = format!(
                    "{details}\n\n操作: 审批重试或放弃 (nuomi evolution rollback {snapshot_id})"
                );
                (title, body)
            }
            Self::CrashRollback {
                crash_count,
                panic_stack,
                restored_version,
            } => {
                let title = format!(
                    "evolution.crash_rollback | 连续崩溃 {crash_count} 次 | 已还原至 {restored_version}"
                );
                let body = format!(
                    "Panic 调用栈:\n{panic_stack}\n\n\
                     已自动还原至上一安全版本 {restored_version}。\n\
                     Safe Mode 启动: nuomi run --safe-mode"
                );
                (title, body)
            }
        }
    }

    /// Event type tag for logging / metrics.
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::Succeeded { .. } => "evolution.succeeded",
            Self::Failed { .. } => "evolution.failed",
            Self::CrashRollback { .. } => "evolution.crash_rollback",
        }
    }
}

/// Dispatches a notification to all enabled sinks. Returns a list of
/// `(sink_kind, error)` pairs for sinks that failed — the caller may log
/// these but delivery is best-effort: partial failures do not propagate.
pub async fn dispatch(
    sinks: &[Arc<dyn OutboundSink>],
    notification: &EvolutionNotification,
) -> Vec<(String, OutboundError)> {
    let (title, body) = notification.format();
    let event_type = notification.event_type();
    let mut failures = Vec::new();
    for sink in sinks {
        match sink.send(&title, &body).await {
            Ok(()) => {
                tracing::info!(
                    event_type,
                    sink_kind = ?sink.kind(),
                    "evolution notification delivered"
                );
            }
            Err(e) => {
                tracing::warn!(
                    event_type,
                    sink_kind = ?sink.kind(),
                    error = %e,
                    "evolution notification delivery failed"
                );
                failures.push((format!("{:?}", sink.kind()), e));
            }
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn succeeded_format() {
        let n = EvolutionNotification::Succeeded {
            from_version: "0.1.0".to_string(),
            to_version: "0.1.1".to_string(),
            layer: EvolutionLayer::L1,
            preference_tags: vec!["terse".to_string(), "dark_mode".to_string()],
            shadow_improvement_pct: Some(12.5),
        };
        let (title, body) = n.format();
        assert!(title.contains("0.1.0 → 0.1.1"));
        assert!(title.contains("L1"));
        assert!(body.contains("terse, dark_mode"));
        assert!(body.contains("+12.5%"));
        assert_eq!(n.event_type(), "evolution.succeeded");
    }

    #[test]
    fn succeeded_no_tags_no_shadow() {
        let n = EvolutionNotification::Succeeded {
            from_version: "0.1.0".to_string(),
            to_version: "0.2.0".to_string(),
            layer: EvolutionLayer::L3,
            preference_tags: Vec::new(),
            shadow_improvement_pct: None,
        };
        let (title, body) = n.format();
        assert!(title.contains("L3"));
        assert!(!body.contains("偏好标签"));
        assert!(body.contains("N/A"));
    }

    #[test]
    fn failed_format() {
        let n = EvolutionNotification::Failed {
            snapshot_id: "snap-abc123".to_string(),
            reason: FailureReason::UnitTestFailed,
            details: "3 tests failed: test_foo, test_bar, test_baz".to_string(),
        };
        let (title, body) = n.format();
        assert!(title.contains("unit_test_failed"));
        assert!(title.contains("snap-abc123"));
        assert!(body.contains("3 tests failed"));
        assert!(body.contains("nuomi evolution rollback snap-abc123"));
        assert_eq!(n.event_type(), "evolution.failed");
    }

    #[test]
    fn crash_rollback_format() {
        let n = EvolutionNotification::CrashRollback {
            crash_count: 3,
            panic_stack: "thread main panicked at src/main.rs:42".to_string(),
            restored_version: "0.1.0".to_string(),
        };
        let (title, body) = n.format();
        assert!(title.contains("连续崩溃 3 次"));
        assert!(title.contains("0.1.0"));
        assert!(body.contains("src/main.rs:42"));
        assert!(body.contains("--safe-mode"));
        assert_eq!(n.event_type(), "evolution.crash_rollback");
    }

    #[test]
    fn failure_reason_as_str() {
        assert_eq!(FailureReason::UnitTestFailed.as_str(), "unit_test_failed");
        assert_eq!(
            FailureReason::PerformanceDegraded.as_str(),
            "performance_degraded"
        );
        assert_eq!(FailureReason::BuildFailed.as_str(), "build_failed");
        assert_eq!(FailureReason::UserRejected.as_str(), "user_rejected");
        assert_eq!(FailureReason::Other.as_str(), "other");
    }

    #[test]
    fn evolution_layer_as_str() {
        assert_eq!(EvolutionLayer::L0.as_str(), "L0");
        assert_eq!(EvolutionLayer::L1.as_str(), "L1");
        assert_eq!(EvolutionLayer::L2.as_str(), "L2");
        assert_eq!(EvolutionLayer::L3.as_str(), "L3");
    }

    #[tokio::test]
    async fn dispatch_to_empty_sinks_is_noop() {
        let n = EvolutionNotification::Succeeded {
            from_version: "0.1.0".to_string(),
            to_version: "0.1.1".to_string(),
            layer: EvolutionLayer::L0,
            preference_tags: Vec::new(),
            shadow_improvement_pct: None,
        };
        let failures = dispatch(&[], &n).await;
        assert!(failures.is_empty());
    }
}
