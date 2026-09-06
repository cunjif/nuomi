//! Gate hooks: pluggable inspection applied to call requests before
//! execution and to results before they are journaled as outputs.
//!
//! Mirrors the style of `evolution/review.rs`: a small trait plus a
//! deliberately heuristic default implementation. An LLM-judged gate can be
//! added later behind the same trait.

use async_trait::async_trait;
use serde_json::Value;

use super::{CallRequest, ResultRecord};

/// Verdict produced by a gate pass.
///
/// `Intercept` is reserved for policy-level blocking (a hook that stops the
/// call for reasons other than malformed input, e.g. an approval workflow).
/// It maps onto [`super::GateOutcome::Intercepted`].
#[derive(Debug, Clone, PartialEq)]
pub enum GateVerdict {
    /// Let the request/output through unchanged.
    Allow,
    /// Rewrite the value with a JSON merge patch (RFC 7386) before
    /// proceeding.
    Fix(Value),
    /// Block and record the reason.
    Reject(String),
    /// Block for policy reasons and record the policy identifier.
    Intercept(String),
}

/// Pluggable inspection of the exchange pipeline. Both directions are
/// mandatory so every gate states its stance on inputs and outputs.
#[async_trait]
pub trait Gate: Send + Sync {
    /// Inspect a call request before the tool executes.
    async fn inspect_input(&self, request: &CallRequest) -> GateVerdict;
    /// Inspect a tool result before it is journaled as the output.
    async fn inspect_output(&self, record: &ResultRecord) -> GateVerdict;
}

/// Heuristic-only default gate: parameter size limit, object shape check,
/// path traversal scan on inputs; size limit on outputs. Behavior is
/// aligned with the P0 review-gate style (reject fast, no side effects).
pub struct DefaultGate {
    max_input_bytes: usize,
    max_output_bytes: usize,
}

impl DefaultGate {
    pub fn new() -> Self {
        Self {
            max_input_bytes: 64 * 1024,
            max_output_bytes: 4 * 1024 * 1024,
        }
    }

    pub fn with_limits(max_input_bytes: usize, max_output_bytes: usize) -> Self {
        Self {
            max_input_bytes,
            max_output_bytes,
        }
    }
}

impl Default for DefaultGate {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns true when any JSON string value contains a `..` path segment
/// (separator-delimited), i.e. a potential path traversal.
fn contains_path_traversal(value: &Value) -> bool {
    match value {
        Value::String(text) => text.split(['/', '\\']).any(|segment| segment == ".."),
        Value::Array(items) => items.iter().any(contains_path_traversal),
        Value::Object(map) => map.values().any(contains_path_traversal),
        _ => false,
    }
}

#[async_trait]
impl Gate for DefaultGate {
    async fn inspect_input(&self, request: &CallRequest) -> GateVerdict {
        let serialized = request.params.to_string();
        let max = self.max_input_bytes;
        if serialized.len() > max {
            return GateVerdict::Reject(format!("parameters exceed {max} bytes"));
        }
        if !request.params.is_object() {
            return GateVerdict::Reject("parameters must be a JSON object".to_string());
        }
        if contains_path_traversal(&request.params) {
            return GateVerdict::Reject(
                "parameters contain path traversal ('..') segments".to_string(),
            );
        }
        GateVerdict::Allow
    }

    async fn inspect_output(&self, record: &ResultRecord) -> GateVerdict {
        let serialized = record.output.to_string();
        let max = self.max_output_bytes;
        if serialized.len() > max {
            return GateVerdict::Reject(format!("output exceeds {max} bytes"));
        }
        GateVerdict::Allow
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    fn request(params: Value) -> CallRequest {
        CallRequest {
            coordinate: crate::exchange::Coordinate {
                run_id: "r".to_string(),
                seq: 1,
                ulid: "ULID".to_string(),
            },
            handle: crate::exchange::CallHandle {
                kind: crate::exchange::HandleKind::Tool,
                id: "fs_read".to_string(),
            },
            params,
        }
    }

    #[tokio::test]
    async fn oversized_input_is_rejected() {
        let gate = DefaultGate::with_limits(8, 1024);
        assert!(matches!(
            gate.inspect_input(&request(json!({ "k": "123456789" })))
                .await,
            GateVerdict::Reject(_)
        ));
    }

    #[tokio::test]
    async fn non_object_input_is_rejected() {
        assert!(matches!(
            gate_in(json!([1, 2, 3])).await,
            GateVerdict::Reject(_)
        ));
    }

    async fn gate_in(params: Value) -> GateVerdict {
        DefaultGate::new().inspect_input(&request(params)).await
    }

    #[tokio::test]
    async fn path_traversal_is_rejected() {
        assert!(matches!(
            gate_in(json!({ "path": "a/../../etc/passwd" })).await,
            GateVerdict::Reject(_)
        ));
        assert!(matches!(
            gate_in(json!({ "path": "..\\secret" })).await,
            GateVerdict::Reject(_)
        ));
        // Dots inside a plain word are fine.
        assert_eq!(
            gate_in(json!({ "path": "a.b/c.txt" })).await,
            GateVerdict::Allow
        );
    }

    #[tokio::test]
    async fn well_formed_input_is_allowed() {
        assert_eq!(
            gate_in(json!({ "path": "src/main.rs" })).await,
            GateVerdict::Allow
        );
    }

    #[tokio::test]
    async fn oversized_output_is_rejected() {
        let gate = DefaultGate::new();
        let record = ResultRecord {
            coordinate: request(json!({})).coordinate,
            handle: request(json!({})).handle,
            output: json!("x".repeat(5 * 1024 * 1024)),
        };
        assert!(matches!(
            gate.inspect_output(&record).await,
            GateVerdict::Reject(_)
        ));
    }

    #[tokio::test]
    async fn gate_is_object_safe() {
        // Gates are stored as Box<dyn Gate> inside the journal.
        let gate: Box<dyn Gate> = Box::new(DefaultGate::new());
        assert_eq!(
            gate.inspect_input(&request(json!({}))).await,
            GateVerdict::Allow
        );
        // Arc<dyn Gate> must stay Send + Sync for the shared journal.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Arc<dyn Gate>>();
    }
}
