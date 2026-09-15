//! Core domain entities. Mirrored 1:1 onto SQLite schema in `migrations/`.
//!
//! Conventions: uuid-v7 string ids, unix-ms i64 `*At` timestamps,
//! JSON payloads as `serde_json::Value`.

use serde::{Deserialize, Serialize};

/// Discriminant for the conversation model: determines UI view and execution path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationKind {
    Chat,
    Group,
    Background,
    Scheduled,
}

impl ConversationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ConversationKind::Chat => "chat",
            ConversationKind::Group => "group",
            ConversationKind::Background => "background",
            ConversationKind::Scheduled => "scheduled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "chat" => Some(ConversationKind::Chat),
            "group" => Some(ConversationKind::Group),
            "background" => Some(ConversationKind::Background),
            "scheduled" => Some(ConversationKind::Scheduled),
            _ => None,
        }
    }
}

/// Whether an agent binding points to a CLI agent profile or a role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRefKind {
    Cli,
    Role,
}

impl AgentRefKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentRefKind::Cli => "cli",
            AgentRefKind::Role => "role",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cli" => Some(AgentRefKind::Cli),
            "role" => Some(AgentRefKind::Role),
            _ => None,
        }
    }
}

/// A resumable conversation / transcript.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub kind: ConversationKind,
    pub agent: Option<(AgentRefKind, String)>,
    pub team_id: Option<String>,
    pub task_id: Option<String>,
    pub schedule_id: Option<String>,
    /// Conversation goal (agent-summarized, user-editable).
    pub goal: Option<String>,
    /// Main agent identifier (e.g. "cli:agent-1" or "role:role-1").
    pub main_agent_id: Option<String>,
    /// Message route mode: "orchestrator_worker" or "master_slave".
    pub route_mode: Option<String>,
    /// Whiteboard route mode: "preemptive" or "concurrent".
    pub whiteboard_route_mode: Option<String>,
}

impl Session {
    /// Convenience constructor for a plain chat session with defaults.
    pub fn new_chat(id: String, title: String, now: i64) -> Self {
        Self {
            id,
            title,
            created_at: now,
            updated_at: now,
            kind: ConversationKind::Chat,
            agent: None,
            team_id: None,
            task_id: None,
            schedule_id: None,
            goal: None,
            main_agent_id: None,
            route_mode: None,
            whiteboard_route_mode: None,
        }
    }
}

/// A todo item belonging to a conversation (agent-summarized, user-editable).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub session_id: String,
    pub description: String,
    pub completed: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Message route mode for multi-agent conversations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RouteMode {
    OrchestratorWorker,
    MasterSlave,
}

impl RouteMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RouteMode::OrchestratorWorker => "orchestrator_worker",
            RouteMode::MasterSlave => "master_slave",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "orchestrator_worker" => Some(RouteMode::OrchestratorWorker),
            "master_slave" => Some(RouteMode::MasterSlave),
            _ => None,
        }
    }
}

/// Whiteboard route mode for group conversations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WhiteboardRouteMode {
    Preemptive,
    Concurrent,
}

impl WhiteboardRouteMode {
    pub fn as_str(self) -> &'static str {
        match self {
            WhiteboardRouteMode::Preemptive => "preemptive",
            WhiteboardRouteMode::Concurrent => "concurrent",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "preemptive" => Some(WhiteboardRouteMode::Preemptive),
            "concurrent" => Some(WhiteboardRouteMode::Concurrent),
            _ => None,
        }
    }
}

/// A participant agent in a conversation (conversation_participants table).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationParticipant {
    pub session_id: String,
    pub agent_kind: AgentRefKind,
    pub agent_ref_id: String,
    pub joined_at: i64,
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

/// Provider-level model/routing settings, persisted under the `"settings"`
/// key inside `ProviderConfig::params` (JSON params extension — no schema
/// change; unknown params keys are preserved).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderSettings {
    /// Model ids exposed by this endpoint, each with per-model capabilities.
    /// Legacy string entries deserialize into `ModelEntry { capabilities:
    /// [reasoning] }` (see [`deserialize_models`]).
    #[serde(default, deserialize_with = "deserialize_models")]
    pub models: Vec<ModelEntry>,
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<i64>,
    #[serde(default)]
    pub timeout_secs: Option<i64>,
    #[serde(default)]
    pub retry: Option<i64>,
    #[serde(default)]
    pub max_concurrency: Option<i64>,
    /// Routing weight (0-10); higher wins when the router picks a master.
    #[serde(default)]
    pub priority: Option<f64>,
    /// Role tags this provider is suited for, e.g. `["code","review"]`.
    #[serde(default)]
    pub roles: Vec<String>,
    /// Per-provider local network proxy for ALL endpoint traffic, e.g.
    /// `http://127.0.0.1:7890` (http/https; socks needs the reqwest `socks`
    /// feature). `None`/empty = direct connection via the shared pool.
    #[serde(default)]
    pub proxy: Option<String>,
    #[serde(default = "crate::domain::entities::provider_enabled_default")]
    pub enabled: bool,
}

fn provider_enabled_default() -> bool {
    true
}

/// A system-level modality capability a model (and transitively a Role)
/// may require. Serialized lowercase (`reasoning`, `image`, `voice`, `video`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    Reasoning,
    Image,
    Voice,
    Video,
}

impl Capability {
    pub const ALL: [Capability; 4] = [
        Capability::Reasoning,
        Capability::Image,
        Capability::Voice,
        Capability::Video,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Capability::Reasoning => "reasoning",
            Capability::Image => "image",
            Capability::Voice => "voice",
            Capability::Video => "video",
        }
    }

    pub fn parse(s: &str) -> Option<Capability> {
        match s {
            "reasoning" => Some(Capability::Reasoning),
            "image" => Some(Capability::Image),
            "voice" => Some(Capability::Voice),
            "video" => Some(Capability::Video),
            _ => None,
        }
    }
}

/// One model exposed by a provider endpoint plus its per-model capabilities
/// (KiloCode-style: each model is individually tagged).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
}

impl ModelEntry {
    pub fn with_caps(id: &str, caps: &[Capability]) -> Self {
        Self {
            id: id.to_string(),
            capabilities: caps.to_vec(),
        }
    }
}

/// Back-compat wire form: old persisted settings stored models as plain id
/// strings. New shape is the [`ModelEntry`] object; unknown string ids map to
/// `ModelEntry { capabilities: [reasoning] }` (documented normalization).
#[derive(Deserialize)]
#[serde(untagged)]
enum ModelEntryRaw {
    Entry(ModelEntry),
    Id(String),
}

fn deserialize_models<'de, D>(deserializer: D) -> Result<Vec<ModelEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = Vec::<ModelEntryRaw>::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .map(|entry| match entry {
            ModelEntryRaw::Entry(entry) => entry,
            ModelEntryRaw::Id(id) => ModelEntry {
                id,
                capabilities: vec![Capability::Reasoning],
            },
        })
        .collect())
}

impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            models: Vec::new(),
            default_model: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            timeout_secs: None,
            retry: None,
            max_concurrency: None,
            priority: None,
            roles: Vec::new(),
            proxy: None,
            enabled: true,
        }
    }
}

impl ProviderSettings {
    /// Plain model ids in list order (display + default-model pickers).
    pub fn model_ids(&self) -> Vec<String> {
        self.models.iter().map(|m| m.id.clone()).collect()
    }

    /// Extracts the `"settings"` key from a provider `params` object;
    /// missing/malformed payloads fall back to defaults.
    pub fn from_params(params: &serde_json::Value) -> Self {
        params
            .get("settings")
            .and_then(|value| serde_json::from_value::<ProviderSettings>(value.clone()).ok())
            .unwrap_or_default()
    }

    /// Writes the settings back into a `params` object, preserving any
    /// pre-existing keys outside the `"settings"` namespace.
    pub fn into_params(self, mut params: serde_json::Value) -> serde_json::Value {
        if !params.is_object() {
            params = serde_json::json!({});
        }
        match serde_json::to_value(&self) {
            Ok(settings) => {
                if let Some(obj) = params.as_object_mut() {
                    obj.insert("settings".into(), settings);
                }
                params
            }
            // ProviderSettings serialization is infallible in practice; on
            // the impossible failure the original params pass through.
            Err(_) => params,
        }
    }
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

impl ProviderConfig {
    /// System capability union of this endpoint: the union over every
    /// configured model's capabilities, plus any legacy free-form
    /// `capabilities` tag that names a system capability (back-compat with
    /// pre-model-entries rows).
    pub fn capability_union(&self) -> Vec<Capability> {
        let settings = ProviderSettings::from_params(&self.params);
        let mut caps: Vec<Capability> = settings
            .models
            .iter()
            .flat_map(|m| m.capabilities.iter().copied())
            .collect();
        for tag in &self.capabilities {
            if let Some(cap) = Capability::parse(tag) {
                if !caps.contains(&cap) {
                    caps.push(cap);
                }
            }
        }
        caps.sort();
        caps.dedup();
        caps
    }

    /// True when [`ProviderConfig::capability_union`] covers every requested
    /// capability.
    pub fn covers(&self, required: &[Capability]) -> bool {
        let caps = self.capability_union();
        required.iter().all(|c| caps.contains(c))
    }

    /// The `enabled` toggle from the persisted settings block.
    pub fn enabled_setting(&self) -> bool {
        ProviderSettings::from_params(&self.params).enabled
    }
}

/// Behavior overlay on top of one or more providers
/// (**Agent = Role + Provider**).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub id: String,
    pub name: String,
    /// Legacy single-provider pin; kept in sync with `provider_ids` (first
    /// entry wins) for pre-existing readers. `None` = unbound / CLI-bound.
    pub provider_id: Option<String>,
    /// Multi-provider bindings (Agent = Role + Provider, migration 0010).
    #[serde(default)]
    pub provider_ids: Vec<String>,
    pub system_prompt_override: Option<String>,
    /// Tool ids this role may call; empty list = unrestricted.
    pub tool_allowlist: Vec<String>,
    /// Modality capabilities this role requires from its bound providers;
    /// enforced at bind time (`role.capability_mismatch`) and used by the
    /// capability router (services/capability_router).
    #[serde(default)]
    pub required_capabilities: Vec<Capability>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i64>,
    pub params: serde_json::Value,
    /// Seeded from the built-in preset catalog (delete is refused).
    #[serde(default)]
    pub builtin: bool,
    /// Produced by the Role Director LLM service (`source` records how).
    #[serde(default)]
    pub generated: bool,
    /// Ephemeral temp role created by the capability router; GC'd after runs.
    #[serde(default)]
    pub ephemeral: bool,
    /// Generation provenance for `generated` roles (description, model, ...).
    #[serde(default)]
    pub source: Option<serde_json::Value>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Role {
    /// Effective capability tags for routing: the typed
    /// `required_capabilities` plus any legacy string tags stored under
    /// `params.capabilities` (pre-0010 router convention).
    pub fn capability_tags(&self) -> Vec<String> {
        let mut tags: Vec<String> = self
            .required_capabilities
            .iter()
            .map(|c| c.as_str().to_string())
            .collect();
        let legacy = self
            .params
            .get("capabilities")
            .and_then(serde_json::Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for tag in legacy {
            if !tags.contains(&tag) {
                tags.push(tag);
            }
        }
        tags
    }
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
    Failed,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Backlog => "backlog",
            TaskStatus::Queued => "queued",
            TaskStatus::Running => "running",
            TaskStatus::Done => "done",
            TaskStatus::Cancelled => "cancelled",
            TaskStatus::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<TaskStatus> {
        match s {
            "backlog" => Some(TaskStatus::Backlog),
            "queued" => Some(TaskStatus::Queued),
            "running" => Some(TaskStatus::Running),
            "done" => Some(TaskStatus::Done),
            "cancelled" => Some(TaskStatus::Cancelled),
            "failed" => Some(TaskStatus::Failed),
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

/// Target type a schedule creates when it fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleTargetKind {
    Task,
    Chat,
    Group,
}

impl ScheduleTargetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ScheduleTargetKind::Task => "task",
            ScheduleTargetKind::Chat => "chat",
            ScheduleTargetKind::Group => "group",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "task" => Some(ScheduleTargetKind::Task),
            "chat" => Some(ScheduleTargetKind::Chat),
            "group" => Some(ScheduleTargetKind::Group),
            _ => None,
        }
    }
}

/// Whether a schedule creates a new session per trigger or reuses one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleSessionMode {
    PerTrigger,
    Reuse,
}

impl ScheduleSessionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ScheduleSessionMode::PerTrigger => "per_trigger",
            ScheduleSessionMode::Reuse => "reuse",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "per_trigger" => Some(ScheduleSessionMode::PerTrigger),
            "reuse" => Some(ScheduleSessionMode::Reuse),
            _ => None,
        }
    }
}

/// A cron/interval trigger that generates queued tasks or conversations.
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
    pub target_kind: ScheduleTargetKind,
    pub agent: Option<(AgentRefKind, String)>,
    pub team_id: Option<String>,
    pub session_mode: ScheduleSessionMode,
    pub session_id: Option<String>,
    pub auto_dispatch: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CliFlavor {
    ClaudeCode,
    Codex,
    Plain,
}

impl CliFlavor {
    pub fn as_str(self) -> &'static str {
        match self {
            CliFlavor::ClaudeCode => "claude_code",
            CliFlavor::Codex => "codex",
            CliFlavor::Plain => "plain",
        }
    }

    pub fn parse(s: &str) -> Option<CliFlavor> {
        match s {
            "claude_code" => Some(CliFlavor::ClaudeCode),
            "codex" => Some(CliFlavor::Codex),
            "plain" => Some(CliFlavor::Plain),
            _ => None,
        }
    }
}

/// An executable external CLI agent (Claude Code / Codex / custom scripts).
/// `args` is a JSON array template supporting a `{prompt}` placeholder;
/// `env` is a JSON object of extra environment variables.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProfile {
    pub id: String,
    pub name: String,
    /// Adapter kind; v1 only supports `cli`.
    pub adapter: String,
    pub flavor: CliFlavor,
    pub command: String,
    pub args: serde_json::Value,
    pub env: serde_json::Value,
    pub working_dir: Option<String>,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationKind {
    FeishuBot,
    QqWebhook,
    Telemetry,
}

impl IntegrationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IntegrationKind::FeishuBot => "feishu_bot",
            IntegrationKind::QqWebhook => "qq_webhook",
            IntegrationKind::Telemetry => "telemetry",
        }
    }

    pub fn parse(s: &str) -> Option<IntegrationKind> {
        match s {
            "feishu_bot" => Some(IntegrationKind::FeishuBot),
            "qq_webhook" => Some(IntegrationKind::QqWebhook),
            "telemetry" => Some(IntegrationKind::Telemetry),
            _ => None,
        }
    }
}

/// An outbound integration endpoint: bot webhook or telemetry receiver
/// (`integrations` table, migration 0004). `config` carries endpoint fields
/// such as `webhook_url`, `secret` (feishu) and `headers`; `events` lists the
/// bus topics the integration subscribes to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Integration {
    pub id: String,
    pub name: String,
    pub kind: IntegrationKind,
    pub config: serde_json::Value,
    pub events: Vec<String>,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Attachment kind — how the file entered the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    File,
    Image,
    Paste,
    Text,
}

impl AttachmentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AttachmentKind::File => "file",
            AttachmentKind::Image => "image",
            AttachmentKind::Paste => "paste",
            AttachmentKind::Text => "text",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "file" => Some(AttachmentKind::File),
            "image" => Some(AttachmentKind::Image),
            "paste" => Some(AttachmentKind::Paste),
            "text" => Some(AttachmentKind::Text),
            _ => None,
        }
    }
}

/// Per-session attachment metadata (content-addressed on disk).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub session_id: String,
    pub seq: Option<i64>,
    pub kind: AttachmentKind,
    pub name: String,
    pub mime: String,
    pub rel_path: String,
    pub size_bytes: i64,
    pub sha256: String,
    pub created_at: i64,
}
