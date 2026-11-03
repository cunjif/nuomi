//! Sandbox domain types (ADR-0022). Wire-compatible with the
//! `sandbox_tasks` table (migration 0037): `authorized_dirs` round-trips as
//! a JSON array column and `limits` as a JSON object column.

use std::fmt;

use serde::{Deserialize, Serialize};

/// What the sandbox runs (CHECK constraint on `sandbox_tasks.task_type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxTaskType {
    /// Execute a code snippet or a file from an authorized directory.
    RunCode,
    /// Build the application itself (the nuomi repo checkout).
    BuildApp,
}

impl SandboxTaskType {
    pub fn as_str(&self) -> &'static str {
        match self {
            SandboxTaskType::RunCode => "run_code",
            SandboxTaskType::BuildApp => "build_app",
        }
    }

    /// Parses the wire/storage form; `None` for unknown kinds.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "run_code" => Some(SandboxTaskType::RunCode),
            "build_app" => Some(SandboxTaskType::BuildApp),
            _ => None,
        }
    }
}

impl fmt::Display for SandboxTaskType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Task lifecycle states (CHECK constraint on `sandbox_tasks.status`).
/// `queued → running → succeeded | failed | timed_out | cancelled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxTaskStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

impl SandboxTaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            SandboxTaskStatus::Queued => "queued",
            SandboxTaskStatus::Running => "running",
            SandboxTaskStatus::Succeeded => "succeeded",
            SandboxTaskStatus::Failed => "failed",
            SandboxTaskStatus::TimedOut => "timed_out",
            SandboxTaskStatus::Cancelled => "cancelled",
        }
    }

    /// Parses the wire/storage form; `None` for unknown states.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "queued" => Some(SandboxTaskStatus::Queued),
            "running" => Some(SandboxTaskStatus::Running),
            "succeeded" => Some(SandboxTaskStatus::Succeeded),
            "failed" => Some(SandboxTaskStatus::Failed),
            "timed_out" => Some(SandboxTaskStatus::TimedOut),
            "cancelled" => Some(SandboxTaskStatus::Cancelled),
            _ => None,
        }
    }

    /// Whether the state is terminal (no further transitions allowed).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            SandboxTaskStatus::Succeeded
                | SandboxTaskStatus::Failed
                | SandboxTaskStatus::TimedOut
                | SandboxTaskStatus::Cancelled
        )
    }
}

impl fmt::Display for SandboxTaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Resource limits for one sandbox task (spec 5.7.1-4 / 6.5-4). Serializes
/// as the `sandbox_tasks.limits` JSON column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxLimits {
    /// Wall-clock ceiling in seconds; exceeded → the task is aborted and
    /// reported as `timed_out`. Defaults to `sandbox_default_timeout_secs`
    /// (120) when the caller does not override.
    pub timeout_secs: u64,
    /// CPU cap in percent (100 = one full core); `None` = executor default.
    pub cpu_percent: Option<u32>,
    /// Memory cap in MB; `None` = executor default.
    pub memory_mb: Option<u32>,
    /// Whether the sandbox may reach the network at all (default false).
    pub network_enabled: bool,
}

/// Build profile for sandbox tasks (ADR-0025 §4.1 Slice 4).
///
/// `Light` is for scenario A and L0–L2 lightweight evolution.
/// `Heavy` is for L3 native full builds with relaxed limits and
/// toolchain cache mounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SandboxProfile {
    /// Default: 120s timeout, 2GB memory, standard isolation.
    #[default]
    Light,
    /// L3 native builds: 1800s timeout, 8GB memory, toolchain cache mount.
    Heavy,
}

impl SandboxProfile {
    pub fn as_str(&self) -> &'static str {
        match self {
            SandboxProfile::Light => "light",
            SandboxProfile::Heavy => "heavy",
        }
    }

    /// Returns the default limits for this profile (ADR-0025 §4.1).
    pub fn default_limits(&self) -> SandboxLimits {
        match self {
            SandboxProfile::Light => SandboxLimits {
                timeout_secs: 120,
                cpu_percent: None,
                memory_mb: Some(2048),
                network_enabled: false,
            },
            SandboxProfile::Heavy => SandboxLimits {
                timeout_secs: 1800,
                cpu_percent: None,
                memory_mb: Some(8192),
                network_enabled: false,
            },
        }
    }
}

impl Default for SandboxLimits {
    fn default() -> Self {
        Self {
            timeout_secs: 120,
            cpu_percent: None,
            memory_mb: None,
            network_enabled: false,
        }
    }
}

impl SandboxLimits {
    /// Validates the limit set (task 7.3 submission check): a zero timeout
    /// or a non-positive percent/memory cap is rejected as `LimitInvalid`.
    pub fn validate(&self) -> Result<(), SandboxError> {
        if self.timeout_secs == 0 {
            return Err(SandboxError::LimitInvalid(
                "timeout_secs must be > 0".to_string(),
            ));
        }
        if let Some(cpu) = self.cpu_percent {
            if cpu == 0 || cpu > 100 * 16 {
                return Err(SandboxError::LimitInvalid(format!(
                    "cpu_percent out of range: {cpu}"
                )));
            }
        }
        if let Some(mem) = self.memory_mb {
            if mem == 0 {
                return Err(SandboxError::LimitInvalid(
                    "memory_mb must be > 0".to_string(),
                ));
            }
        }
        Ok(())
    }
}

/// A runtime declared by a sandbox plugin (spec 5.7.1-6a: the plugin
/// documents its isolation mechanism and supported runtime list).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxRuntimeInfo {
    /// Runtime id used in `SandboxTaskSpec::runtime` (e.g. `"python"`).
    pub id: String,
    /// Human-readable display name.
    pub name: String,
    /// Isolation mechanism description surfaced in the UI.
    pub isolation: String,
}

/// What to run in the sandbox — the submission contract (spec 5.7.1-2/3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxTaskSpec {
    pub task_type: SandboxTaskType,
    /// Submitter / source identifier (ADR-0026 §1, spec 5.7.1-5):
    /// `"workbench"` / `"selfeval"` / `"repl"` / future user id. Stored
    /// on the `sandbox_tasks.submitter` column and carried in every
    /// `sandbox.task_state_changed` audit event payload.
    pub submitter: String,
    /// Executor runtime id (must appear in the plugin's runtime list).
    pub runtime: String,
    /// Code snippet, target file path (inside an authorized dir), or build
    /// target description — stored verbatim in `sandbox_tasks.source`.
    pub source: String,
    /// Directory allowlist (JSON array column; kernel-enforced subset of
    /// the current workspace's work directories, ADR-0021 semantics).
    pub authorized_dirs: Vec<String>,
    /// Application-level ephemeral temp dirs the sandbox may access
    /// (ADR-0025 §4.2 Slice 2). Each path must reside under the
    /// ephemeral root (`<app_data_dir>/ephemeral/nuomi-selfeval-*`).
    /// The workbench leaves this empty; the self-evolution engine
    /// populates it when submitting assessment tasks. Validated in
    /// `submit_check` as a kernel-enforced convergence boundary.
    pub app_temp_dirs: Vec<String>,
    /// Build profile (ADR-0025 §4.1 Slice 4). Determines default limits
    /// when the caller does not override them explicitly.
    pub profile: SandboxProfile,
    pub limits: SandboxLimits,
}

/// Resource usage snapshot reported with a result (spec 5.7.1-5).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxResourceUsage {
    /// CPU time consumed, if the executor reports it.
    pub cpu_time_ms: Option<u64>,
    /// Peak resident memory in MB, if the executor reports it.
    pub peak_memory_mb: Option<u64>,
}

/// Final result of a sandbox task (spec 5.7.1-2: stdout/stderr/status/
/// duration plus artifact location and resource usage).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxTaskResult {
    pub status: SandboxTaskStatus,
    pub stdout: String,
    pub stderr: String,
    /// Host-local path of the full stdout when `stdout_truncated` is true
    /// (ADR-0026 §4 / ADR-0022 §6: large outputs live under
    /// `.nuomi/sandbox/<task_id>/stdout.log`; the DB row keeps the
    /// inline prefix plus this ref). `None` when the inline `stdout` is
    /// the complete output.
    pub stdout_ref: Option<String>,
    pub stderr_ref: Option<String>,
    /// Whether the inline `stdout` was capped at `INLINE_OUTPUT_CAP` and
    /// the full text was spilled to `stdout_ref`.
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// Exit code / signal / abort reason (exit_info column).
    pub exit_info: Option<String>,
    pub duration_ms: Option<u64>,
    /// Artifact location once fetched (sandbox path or host-local path).
    pub artifact_ref: Option<String>,
    /// Whether the artifact is still retrievable from the sandbox for a
    /// retry (artifact_retryable column).
    pub artifact_retryable: bool,
    pub usage: SandboxResourceUsage,
}

/// Sandbox-domain errors. The four `unavailable` / `dir_not_authorized` /
/// `runtime_unknown` / `limit_invalid` variants map 1:1 onto the IPC error
/// codes of task 7.6.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxError {
    /// The sandbox plugin is disabled or the executor environment cannot
    /// be reached (spec 5.7.3-1).
    Unavailable(String),
    /// A requested directory is outside the current workspace's work
    /// directories (kernel-enforced convergence, ADR-0022 decision 3).
    DirNotAuthorized(String),
    /// The runtime id is not in the plugin's declared list.
    RuntimeUnknown(String),
    /// The limit set failed validation.
    LimitInvalid(String),
    /// No task with the given id.
    TaskNotFound(String),
    /// Anything else (executor I/O, serialization, ...).
    Internal(String),
}

impl SandboxError {
    /// Stable IPC error code (task 7.6 mapping).
    pub fn code(&self) -> &'static str {
        match self {
            SandboxError::Unavailable(_) => "sandbox.unavailable",
            SandboxError::DirNotAuthorized(_) => "sandbox.dir_not_authorized",
            SandboxError::RuntimeUnknown(_) => "sandbox.runtime_unknown",
            SandboxError::LimitInvalid(_) => "sandbox.limit_invalid",
            SandboxError::TaskNotFound(_) => "sandbox.task_not_found",
            SandboxError::Internal(_) => "sandbox.internal",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            SandboxError::Unavailable(m)
            | SandboxError::DirNotAuthorized(m)
            | SandboxError::RuntimeUnknown(m)
            | SandboxError::LimitInvalid(m)
            | SandboxError::TaskNotFound(m)
            | SandboxError::Internal(m) => m,
        }
    }
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for SandboxError {}

// ---- secret payload sniffing (ADR-0026 §2, spec 5.7.1-7) ---------------

/// Inline output cap (ADR-0022 §6 / ADR-0026 §4): stdout/stderr up to this
/// many bytes stay in the `sandbox_tasks` row; beyond it the full text is
/// spilled to `.nuomi/sandbox/<task_id>/{stdout,stderr}.log` and the row
/// keeps the prefix plus `stdout_ref` / `stderr_ref`.
pub const INLINE_OUTPUT_CAP: usize = 64 * 1024;

/// Compiled secret-detection patterns (ADR-0026 §2). Each entry is a
/// `(label, regex)` pair; `check_secret_payload` returns the first hit.
///
/// The patterns are intentionally conservative to minimise false positives
/// on legitimate code while catching common accidental secret leakage:
/// - `keyring://` and `os.keyring` / `keyring.get` — explicit keyring access
/// - `Bearer <20+ word chars>` — inlined API bearer token
/// - `API_KEY=` / `SECRET=` / `TOKEN=` / `PASSWORD=` (case-insensitive) —
///   secret env-var assignment
/// - `AKIA[0-9A-Z]{16}` — AWS access key id format
/// - `-----BEGIN (RSA|EC|OPENSSH|PGP) PRIVATE KEY-----` — PEM private key
static SECRET_PATTERNS: &[(&str, &str)] = &[
    ("keyring reference", r"keyring://"),
    ("keyring api access", r"(?i)os\.keyring|keyring\.get"),
    ("bearer token", r"Bearer\s+[A-Za-z0-9\-_]{20,}"),
    ("secret env assignment", r"(?i)(API_KEY|SECRET|TOKEN|PASSWORD)\s*="),
    ("aws access key id", r"AKIA[0-9A-Z]{16}"),
    ("pem private key", r"-----BEGIN (RSA|EC|OPENSSH|PGP) PRIVATE KEY-----"),
];

/// Scans `text` for known secret-leak patterns (ADR-0026 §2). Returns the
/// label of the first hit. Used by `submit_check` to reject submissions
/// whose `source` or `authorized_dirs` carry secrets before they reach the
/// executor — a kernel-side backstop that does not rely on the executor's
/// isolation boundary.
pub fn check_secret_payload(text: &str) -> Option<&'static str> {
    for (label, pattern) in SECRET_PATTERNS {
        if regex::Regex::new(pattern)
            .expect("SECRET_PATTERNS must compile")
            .is_match(text)
        {
            return Some(label);
        }
    }
    None
}

// ---- JSON column round-trips (0037 interop, task 7.2 unit tests) --------

/// Serializes the directory allowlist as the `authorized_dirs` JSON column.
pub fn dirs_to_json(dirs: &[String]) -> Result<String, SandboxError> {
    serde_json::to_string(dirs).map_err(|e| SandboxError::Internal(e.to_string()))
}

/// Parses the `authorized_dirs` JSON column back into a directory list.
pub fn dirs_from_json(s: &str) -> Result<Vec<String>, SandboxError> {
    serde_json::from_str(s).map_err(|e| SandboxError::Internal(e.to_string()))
}

impl SandboxLimits {
    /// Serializes as the `limits` JSON column.
    pub fn to_json(&self) -> Result<String, SandboxError> {
        serde_json::to_string(self).map_err(|e| SandboxError::Internal(e.to_string()))
    }

    /// Parses the `limits` JSON column.
    pub fn from_json(s: &str) -> Result<Self, SandboxError> {
        serde_json::from_str(s).map_err(|e| SandboxError::Internal(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_type_round_trips_storage_strings() {
        for t in [SandboxTaskType::RunCode, SandboxTaskType::BuildApp] {
            assert_eq!(SandboxTaskType::parse(t.as_str()), Some(t));
        }
        assert_eq!(SandboxTaskType::parse("evil"), None);
    }

    #[test]
    fn status_round_trips_storage_strings_and_terminal_set() {
        for s in [
            SandboxTaskStatus::Queued,
            SandboxTaskStatus::Running,
            SandboxTaskStatus::Succeeded,
            SandboxTaskStatus::Failed,
            SandboxTaskStatus::TimedOut,
            SandboxTaskStatus::Cancelled,
        ] {
            assert_eq!(SandboxTaskStatus::parse(s.as_str()), Some(s));
        }
        assert_eq!(SandboxTaskStatus::parse("paused"), None);
        assert!(!SandboxTaskStatus::Queued.is_terminal());
        assert!(!SandboxTaskStatus::Running.is_terminal());
        assert!(SandboxTaskStatus::Succeeded.is_terminal());
        assert!(SandboxTaskStatus::Failed.is_terminal());
        assert!(SandboxTaskStatus::TimedOut.is_terminal());
        assert!(SandboxTaskStatus::Cancelled.is_terminal());
    }

    #[test]
    fn authorized_dirs_json_column_round_trip() {
        let dirs = vec!["C:\\proj\\alpha".to_string(), "/home/u/nuomi".to_string()];
        let json = dirs_to_json(&dirs).unwrap();
        assert_eq!(json, r#"["C:\\proj\\alpha","/home/u/nuomi"]"#);
        assert_eq!(dirs_from_json(&json).unwrap(), dirs);
        assert!(dirs_to_json(&[]).unwrap() == "[]");
    }

    #[test]
    fn limits_json_column_round_trip_with_defaults() {
        let limits = SandboxLimits::default();
        let json = limits.to_json().unwrap();
        assert_eq!(
            json,
            r#"{"timeout_secs":120,"cpu_percent":null,"memory_mb":null,"network_enabled":false}"#
        );
        let back = SandboxLimits::from_json(&json).unwrap();
        assert_eq!(back, limits);

        let full = SandboxLimits {
            timeout_secs: 300,
            cpu_percent: Some(50),
            memory_mb: Some(2048),
            network_enabled: true,
        };
        let json = full.to_json().unwrap();
        assert_eq!(SandboxLimits::from_json(&json).unwrap(), full);
    }

    #[test]
    fn limits_validation_rejects_zero_and_out_of_range() {
        assert!(SandboxLimits::default().validate().is_ok());

        let zero_timeout = SandboxLimits {
            timeout_secs: 0,
            ..SandboxLimits::default()
        };
        assert!(matches!(
            zero_timeout.validate(),
            Err(SandboxError::LimitInvalid(_))
        ));

        let bad_cpu = SandboxLimits {
            cpu_percent: Some(0),
            ..SandboxLimits::default()
        };
        assert!(matches!(
            bad_cpu.validate(),
            Err(SandboxError::LimitInvalid(_))
        ));

        let bad_mem = SandboxLimits {
            memory_mb: Some(0),
            ..SandboxLimits::default()
        };
        assert!(matches!(
            bad_mem.validate(),
            Err(SandboxError::LimitInvalid(_))
        ));
    }

    #[test]
    fn error_codes_map_one_to_one_to_ipc_contract() {
        assert_eq!(
            SandboxError::Unavailable("x".into()).code(),
            "sandbox.unavailable"
        );
        assert_eq!(
            SandboxError::DirNotAuthorized("x".into()).code(),
            "sandbox.dir_not_authorized"
        );
        assert_eq!(
            SandboxError::RuntimeUnknown("x".into()).code(),
            "sandbox.runtime_unknown"
        );
        assert_eq!(
            SandboxError::LimitInvalid("x".into()).code(),
            "sandbox.limit_invalid"
        );
        assert_eq!(
            SandboxError::TaskNotFound("x".into()).code(),
            "sandbox.task_not_found"
        );
        assert_eq!(
            SandboxError::Internal("x".into()).code(),
            "sandbox.internal"
        );
    }

    #[test]
    fn secret_sniffer_hits_each_known_pattern() {
        // keyring reference
        assert_eq!(
            check_secret_payload("import os; os.environ['K'] = 'keyring://system/nuomi'"),
            Some("keyring reference")
        );
        // keyring api access
        assert_eq!(
            check_secret_payload("val = os.keyring.get_password('nuomi', 'api')"),
            Some("keyring api access")
        );
        // bearer token (20+ word chars after "Bearer ")
        assert_eq!(
            check_secret_payload("Authorization: Bearer abcdefghijklmnopqrstuv"),
            Some("bearer token")
        );
        // secret env assignment (case-insensitive)
        assert_eq!(
            check_secret_payload("api_key=sk-live-1234567890"),
            Some("secret env assignment")
        );
        assert_eq!(
            check_secret_payload("TOKEN=eyJhbGciOiJIUzI1NiJ9"),
            Some("secret env assignment")
        );
        // aws access key id
        assert_eq!(
            check_secret_payload("aws_access_key_id = AKIAIOSFODNN7EXAMPLE"),
            Some("aws access key id")
        );
        // pem private key
        assert_eq!(
            check_secret_payload("-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAK"),
            Some("pem private key")
        );
    }

    #[test]
    fn secret_sniffer_passes_legitimate_code_and_build_targets() {
        // Plain Python snippet.
        assert_eq!(check_secret_payload("print('hello world')"), None);
        // Build target description.
        assert_eq!(check_secret_payload("cargo build --release -p nuomi-cli"), None);
        // File path inside an authorized dir.
        assert_eq!(
            check_secret_payload("C:\\proj\\nuomi\\src\\main.rs"),
            None
        );
        // Short bearer-like string below the 20-char threshold (false-positive guard).
        assert_eq!(check_secret_payload("Bearer short"), None);
        // Variable name containing "key" but not the assignment pattern.
        assert_eq!(check_secret_payload("let keymap = Keymap::default();"), None);
        // "API_KEY" as a read reference, not an assignment.
        assert_eq!(check_secret_payload("std::env::var(\"API_KEY\")"), None);
    }

    #[test]
    fn inline_output_cap_is_64kb() {
        assert_eq!(INLINE_OUTPUT_CAP, 65_536);
    }
}
