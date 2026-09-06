//! Exchange filesystem: tool I/O flows through a journaled exchange layer
//! instead of flowing directly between the agent and the tool.
//!
//! When an agent calls a tool/MCP/plugin/skill, the call is first written to
//! the exchange journal as a coordinate record (unique coordinate, handle,
//! parameters). An input gate validates / reviews / fixes / intercepts the
//! call before execution. The output is likewise journaled behind an output
//! gate; the agent only receives an execution receipt (coordinate + status +
//! preview) and later reads the full output back with retrieval commands
//! (`exchange_list` / `exchange_read` / `exchange_tail`), like paging through
//! a document.
//!
//! Storage is a three-tier model (see docs/adr/0007-exchange-filesystem.md):
//! 1. Hot tier — in-memory index, zero-IO reads for fresh records.
//! 2. Durable tier — background writer group-commits records to per-run
//!    JSONL segment files (one `write_all` + `flush` per batch, never a
//!    per-record fsync).
//! 3. Spill tier — oversized payloads live in content-addressed blob files
//!    (hash-deduplicated); the index keeps only an envelope.

pub mod gate;
pub mod journal;
pub mod ulid;

pub use gate::{DefaultGate, Gate, GateVerdict};
pub use journal::ExchangeJournal;

use serde::{Deserialize, Serialize};
use std::fmt;

/// Errors produced by the exchange journal.
#[derive(Debug, thiserror::Error)]
pub enum ExchangeError {
    #[error("exchange coordinate '{0}' not found")]
    CoordinateNotFound(String),
    #[error("record '{0}' is not awaiting completion")]
    NotPending(String),
    #[error("exchange journal is closed")]
    Closed,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Kind of callable target behind a coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleKind {
    Tool,
    Mcp,
    Plugin,
    Skill,
}

/// Identifies the target of an exchanged call: its kind and registry id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallHandle {
    pub kind: HandleKind,
    pub id: String,
}

/// Unique call coordinate: `exch/<run_id>/<seq>-<ulid>`.
/// `seq` is monotonic per run and allocated by the journal; `ulid` is a
/// time-sortable unique id making coordinates collision-free across runs.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Coordinate {
    pub run_id: String,
    pub seq: u64,
    pub ulid: String,
}

impl Coordinate {
    /// Parses a coordinate from its display form `exch/<run_id>/<seq>-<ulid>`.
    pub fn parse(text: &str) -> Option<Coordinate> {
        let rest = text.strip_prefix("exch/")?;
        let (run_id, tail) = rest.rsplit_once('/')?;
        let (seq_text, ulid) = tail.split_once('-')?;
        Some(Coordinate {
            run_id: run_id.to_string(),
            seq: seq_text.parse().ok()?,
            ulid: ulid.to_string(),
        })
    }
}

impl fmt::Display for Coordinate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "exch/{}/{seq}-{}",
            self.run_id,
            self.ulid,
            seq = self.seq
        )
    }
}

/// Lifecycle status of an exchange record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordStatus {
    /// Gate allowed the call; awaiting completion.
    Pending,
    /// Executed and journaled successfully.
    Succeeded,
    /// Execution failed, or the output gate rejected the result.
    Failed,
    /// The input gate rejected the call before execution.
    Rejected,
    /// A policy hook intercepted the call before execution.
    Intercepted,
}

impl RecordStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            RecordStatus::Pending => "pending",
            RecordStatus::Succeeded => "succeeded",
            RecordStatus::Failed => "failed",
            RecordStatus::Rejected => "rejected",
            RecordStatus::Intercepted => "intercepted",
        }
    }

    /// Parses a status from its snake_case form (tool-argument friendly).
    pub fn parse(text: &str) -> Option<RecordStatus> {
        match text {
            "pending" => Some(RecordStatus::Pending),
            "succeeded" => Some(RecordStatus::Succeeded),
            "failed" => Some(RecordStatus::Failed),
            "rejected" => Some(RecordStatus::Rejected),
            "intercepted" => Some(RecordStatus::Intercepted),
            _ => None,
        }
    }
}

/// A full exchange record: the journaled unit of one tool call.
/// `Record` doubles as the return type of `read` — when the payload was
/// spilled (or reclaimed by GC) `output` is `None` and the envelope fields
/// (`preview`, `output_len`, `output_hash`) describe the blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub coordinate: Coordinate,
    pub handle: CallHandle,
    pub params: serde_json::Value,
    /// Input merge patch applied by the gate (`Fix` verdict).
    pub patch: Option<serde_json::Value>,
    /// Output merge patch applied by the gate (`Fix` verdict).
    pub output_patch: Option<serde_json::Value>,
    pub status: RecordStatus,
    pub output: Option<serde_json::Value>,
    pub output_spilled: bool,
    pub output_len: Option<usize>,
    pub output_hash: Option<String>,
    pub preview: Option<String>,
    /// Rejection reason, interception policy, or failure message.
    pub reason: Option<String>,
    pub created_at_ms: u64,
    pub completed_at_ms: Option<u64>,
}

/// Lightweight index entry: everything about a record except its payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope {
    pub coordinate: Coordinate,
    pub handle: CallHandle,
    pub status: RecordStatus,
    pub output_spilled: bool,
    pub output_len: Option<usize>,
    pub output_hash: Option<String>,
    pub preview: Option<String>,
    pub reason: Option<String>,
    pub created_at_ms: u64,
    pub completed_at_ms: Option<u64>,
}

impl Envelope {
    pub fn from_record(record: &Record) -> Envelope {
        Envelope {
            coordinate: record.coordinate.clone(),
            handle: record.handle.clone(),
            status: record.status,
            output_spilled: record.output_spilled,
            output_len: record.output_len,
            output_hash: record.output_hash.clone(),
            preview: record.preview.clone(),
            reason: record.reason.clone(),
            created_at_ms: record.created_at_ms,
            completed_at_ms: record.completed_at_ms,
        }
    }
}

/// Optional filters for `list`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListFilter {
    /// Matches `handle.id` (any handle kind).
    pub tool: Option<String>,
    pub status: Option<RecordStatus>,
}

impl ListFilter {
    fn matches(&self, record: &Record) -> bool {
        if let Some(tool) = &self.tool {
            if record.handle.id != *tool {
                return false;
            }
        }
        if let Some(status) = &self.status {
            if record.status != *status {
                return false;
            }
        }
        true
    }
}

/// Request presented to the input gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallRequest {
    pub coordinate: Coordinate,
    pub handle: CallHandle,
    pub params: serde_json::Value,
}

/// Result presented to the output gate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultRecord {
    pub coordinate: Coordinate,
    pub handle: CallHandle,
    pub output: serde_json::Value,
}

/// Outcome of submitting a call through the exchange gate.
#[derive(Debug, Clone)]
pub enum GateOutcome {
    /// Gate allowed the call; execute and later `complete(coord, output)`.
    Allowed(Coordinate),
    /// Gate patched the parameters; execute with the patched params.
    Fixed {
        coordinate: Coordinate,
        patch: serde_json::Value,
    },
    /// Gate rejected the call; do not execute.
    Rejected {
        coordinate: Coordinate,
        reason: String,
    },
    /// A policy hook intercepted the call; do not execute.
    Intercepted {
        coordinate: Coordinate,
        policy: String,
    },
}

impl GateOutcome {
    pub fn coordinate(&self) -> &Coordinate {
        match self {
            GateOutcome::Allowed(coordinate) => coordinate,
            GateOutcome::Fixed { coordinate, .. } => coordinate,
            GateOutcome::Rejected { coordinate, .. } => coordinate,
            GateOutcome::Intercepted { coordinate, .. } => coordinate,
        }
    }
}

/// Journal tuning. Defaults implement the ADR 0007 three-tier model.
#[derive(Debug, Clone)]
pub struct ExchangeConfig {
    /// Group-commit window: the background writer flushes at least this
    /// often (crash window for "executed but not yet journaled" records).
    pub flush_interval_ms: u64,
    /// Group-commit backlog threshold: flush as soon as this many records
    /// are queued.
    pub flush_batch_min: usize,
    /// Payloads serialized larger than this spill to content-addressed
    /// blob files; the index keeps only an envelope.
    pub spill_threshold_bytes: usize,
    /// Envelope preview length in bytes.
    pub preview_bytes: usize,
    /// Maximum inline payloads kept per run in the hot tier; older terminal
    /// records are demoted to envelope-only (payload stays on disk).
    pub max_hot_payloads_per_run: usize,
    /// Retention policy applied by `gc`.
    pub retain_policy: RetainPolicy,
}

impl Default for ExchangeConfig {
    fn default() -> Self {
        Self {
            flush_interval_ms: 50,
            flush_batch_min: 32,
            spill_threshold_bytes: 8 * 1024,
            preview_bytes: 256,
            max_hot_payloads_per_run: 256,
            retain_policy: RetainPolicy::DropPayloads,
        }
    }
}

/// Capacity governance policy (ADR 0007 §capacity).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetainPolicy {
    /// Keep everything forever.
    KeepAll,
    /// Keep the N most recently active runs; delete older runs entirely.
    KeepLastNRuns(usize),
    /// Default: keep envelopes in the index, drop payloads (blobs and
    /// inline outputs) at run end.
    #[default]
    DropPayloads,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coordinate_display_and_parse_roundtrip() {
        let coordinate = Coordinate {
            run_id: "run-42".to_string(),
            seq: 7,
            ulid: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
        };
        let text = coordinate.to_string();
        assert_eq!(text, "exch/run-42/7-01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert_eq!(Coordinate::parse(&text), Some(coordinate));
    }

    #[test]
    fn coordinate_parse_rejects_garbage() {
        assert!(Coordinate::parse("nope").is_none());
        assert!(Coordinate::parse("exch/run/abc-ulid").is_none());
    }

    #[test]
    fn list_filter_matches_tool_and_status() {
        let record = Record {
            coordinate: Coordinate {
                run_id: "r".to_string(),
                seq: 1,
                ulid: "U".to_string(),
            },
            handle: CallHandle {
                kind: HandleKind::Tool,
                id: "echo".to_string(),
            },
            params: serde_json::json!({}),
            patch: None,
            output_patch: None,
            status: RecordStatus::Succeeded,
            output: None,
            output_spilled: false,
            output_len: None,
            output_hash: None,
            preview: None,
            reason: None,
            created_at_ms: 0,
            completed_at_ms: None,
        };
        let filter = ListFilter {
            tool: Some("echo".to_string()),
            status: Some(RecordStatus::Succeeded),
        };
        assert!(filter.matches(&record));
        let filter_miss = ListFilter {
            tool: Some("other".to_string()),
            status: None,
        };
        assert!(!filter_miss.matches(&record));
    }
}
