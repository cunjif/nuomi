//! Core domain entities. Mirrored 1:1 onto SQLite schema in `migrations/`.
//!
//! Conventions: uuid-v7 string ids, unix-ms i64 `*At` timestamps,
//! JSON payloads as `serde_json::Value`.

use serde::{Deserialize, Serialize};

/// A resumable conversation / transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Append-only event log record (`events` table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    /// Database-assigned monotonic row id.
    pub id: i64,
    pub aggregate_type: String,
    pub aggregate_id: String,
    /// e.g. `thought`, `tool_call`, `tool_result`, `message`, `state_changed`, `usage`.
    pub kind: String,
    pub payload: serde_json::Value,
    /// Monotonic within `(aggregate_type, aggregate_id)` — used for gap recovery.
    pub seq: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderProtocol {
    OpenAiCompatible,
    AnthropicCompatible,
}

/// A model service endpoint (master or slave).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub protocol: ProviderProtocol,
    pub base_url: String,
    /// Reference into the OS keyring (never the key itself).
    pub keyring_ref: Option<String>,
    /// Capability tags used by master-slave routing, e.g. `["code","fast"]`.
    pub capabilities: Vec<String>,
    pub is_master: bool,
    /// Position in the slave fallback chain (lower first); `None` = not chained.
    pub fallback_order: Option<i64>,
    /// Protocol-level params (temperature defaults etc.).
    pub params: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Behavior overlay on top of a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub id: String,
    pub name: String,
    pub provider_id: Option<String>,
    pub system_prompt_override: Option<String>,
    /// Tool ids this role may call; empty list = unrestricted.
    pub tool_allowlist: Vec<String>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i64>,
    pub params: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamTopology {
    Pipeline,
    Router,
    GroupChat,
}

/// A composition of roles plus collaboration topology.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Team {
    pub id: String,
    pub name: String,
    pub topology: TeamTopology,
    /// Member role ids in pipeline order / membership order.
    pub member_role_ids: Vec<String>,
    /// Topology-specific config (max_rounds, selector settings, ...).
    pub config: serde_json::Value,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Cross-session long-term memory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub source_session_id: Option<String>,
    pub tags: Vec<String>,
    /// Free-form classification, e.g. `note`, `user_profile`, `preference`.
    pub kind: String,
    /// Marks user-profile facts that evolution must always consider.
    pub user_profile: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Shared group-chat blackboard entry (append-only).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhiteBoardNote {
    pub id: String,
    pub session_id: String,
    pub author_role_id: Option<String>,
    /// Structured note type, e.g. `finding`, `decision`, `question`, `artifact_ref`.
    pub note_type: String,
    pub body: String,
    pub refs: serde_json::Value,
    pub seq: i64,
    pub created_at: i64,
}

/// Board-level task status (kanban column). Distinct from [`RunState`]:
/// a Session spawns Tasks, each Task is executed by one or more Runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Backlog,
    Queued,
    Running,
    Done,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Backlog => "backlog",
            TaskStatus::Queued => "queued",
            TaskStatus::Running => "running",
            TaskStatus::Done => "done",
            TaskStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<TaskStatus> {
        match s {
            "backlog" => Some(TaskStatus::Backlog),
            "queued" => Some(TaskStatus::Queued),
            "running" => Some(TaskStatus::Running),
            "done" => Some(TaskStatus::Done),
            "cancelled" => Some(TaskStatus::Cancelled),
            _ => None,
        }
    }
}

/// A unit of work created by a session (or the scheduler); executed by runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub session_id: Option<String>,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One execution attempt of a task; lifecycle governed by
/// [`crate::domain::run_state::RunState`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub task_id: String,
    pub session_id: String,
    pub status: crate::domain::run_state::RunState,
    /// Unix-ms of the last heartbeat (orphan detection compares against it).
    pub heartbeat_at: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Pending,
    Approved,
    Denied,
}

impl ApprovalDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            ApprovalDecision::Pending => "pending",
            ApprovalDecision::Approved => "approved",
            ApprovalDecision::Denied => "denied",
        }
    }

    pub fn parse(s: &str) -> Option<ApprovalDecision> {
        match s {
            "pending" => Some(ApprovalDecision::Pending),
            "approved" => Some(ApprovalDecision::Approved),
            "denied" => Some(ApprovalDecision::Denied),
            _ => None,
        }
    }
}

/// A sensitive-tool call awaiting a human decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approval {
    pub id: String,
    pub run_id: String,
    pub tool_name: String,
    pub arguments_json: String,
    pub decision: ApprovalDecision,
    pub decided_at: Option<i64>,
    pub created_at: i64,
}

/// A cron/interval trigger that generates queued tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub id: String,
    pub name: String,
    /// 5-field cron subset or `@every <seconds>` (see scheduler_service).
    pub cron_expr: String,
    pub task_title: String,
    pub task_description: String,
    pub enabled: bool,
    pub last_triggered_at: Option<i64>,
    pub next_trigger_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptStatus {
    Candidate,
    Active,
    Retired,
}

/// Versioned system-prompt record produced by the evolution engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptVersion {
    pub id: String,
    /// Prompt slot name, default `"system_prompt"`; evolution may version others.
    pub plugin: String,
    pub version: i64,
    pub status: PromptStatus,
    pub content: String,
    /// Human/LLM-readable diff against `parent_version`.
    pub diff_text: Option<String>,
    pub parent_version: Option<i64>,
    pub activated_at: Option<i64>,
    pub created_at: i64,
}
