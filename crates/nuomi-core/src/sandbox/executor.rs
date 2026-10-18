//! The `SandboxExecutor` contract (ADR-0022): the builtin sandbox plugin
//! shell delegates every execution concern here.
//!
//! Implementations enforce the isolation boundary themselves — container
//! or strongly isolated process; manifest declarations are display-only
//! and never a security boundary (ADR-0022 §2). The default
//! implementation (cube_executor.rs, task 7.4) talks to a
//! CubeSandbox-class runtime as an independent process.
//!
//! Task ids are kernel-minted (the `sandbox_tasks` row id), so poll /
//! cancel / fetch calls share the id space with the audit log.

use async_trait::async_trait;

use super::types::{
    SandboxError, SandboxResourceUsage, SandboxRuntimeInfo, SandboxTaskSpec, SandboxTaskStatus,
};

/// State snapshot returned by [`SandboxExecutor::poll`]. Carries output
/// totals (byte offsets into the decoded streams) so the task manager can
/// pull increments via [`SandboxExecutor::fetch_output`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxTaskSnapshot {
    pub status: SandboxTaskStatus,
    /// Total stdout bytes produced so far.
    pub stdout_total: u64,
    /// Total stderr bytes produced so far.
    pub stderr_total: u64,
    /// Whether the executor truncated stdout at its inline cap (the full
    /// copy stays in the task's artifact/output directory, ADR-0022 §6).
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// Resource usage so far, when the executor reports it.
    pub usage: Option<SandboxResourceUsage>,
    /// Terminal detail: exit code, signal, or abort reason.
    pub exit_info: Option<String>,
    /// Wall-clock duration once terminal.
    pub duration_ms: Option<u64>,
    /// Whether an artifact is retrievable via [`SandboxExecutor::fetch_artifact`].
    pub artifact_available: bool,
}

impl SandboxTaskSnapshot {
    /// A `queued` snapshot with no output yet.
    pub fn queued() -> Self {
        Self {
            status: SandboxTaskStatus::Queued,
            stdout_total: 0,
            stderr_total: 0,
            stdout_truncated: false,
            stderr_truncated: false,
            usage: None,
            exit_info: None,
            duration_ms: None,
            artifact_available: false,
        }
    }

    /// A `running` snapshot; executors fill in the live output totals.
    pub fn running() -> Self {
        Self {
            status: SandboxTaskStatus::Running,
            ..Self::queued()
        }
    }

    /// A terminal snapshot (`succeeded` / `failed` / `timed_out` /
    /// `cancelled`) with exit detail and duration.
    pub fn terminal(
        status: SandboxTaskStatus,
        exit_info: Option<String>,
        duration_ms: Option<u64>,
    ) -> Self {
        Self {
            status,
            exit_info,
            duration_ms,
            ..Self::queued()
        }
    }
}

/// One incremental output chunk of the cursor protocol. The caller keeps
/// the last-seen `total` as its cursor and passes it to the next
/// [`SandboxExecutor::fetch_output`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxOutputChunk {
    /// Byte offset this chunk starts at (the cursor passed in).
    pub start: u64,
    /// UTF-8 text of the increment.
    pub text: String,
    /// Total stream length at fetch time — the caller's next cursor.
    pub total: u64,
    /// Whether the executor truncated the stream at its inline cap.
    pub truncated: bool,
}

impl SandboxOutputChunk {
    /// An empty chunk parked at the given cursor (no new output).
    pub fn empty_at(cursor: u64) -> Self {
        Self {
            start: cursor,
            text: String::new(),
            total: cursor,
            truncated: false,
        }
    }
}

/// An artifact fetched back from the sandbox (ADR-0022 §6: large outputs
/// live under `.nuomi/sandbox/<task_id>/`; the DB row keeps only the ref).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxArtifact {
    /// Artifact name / relative path inside the task's artifact dir.
    pub name: String,
    /// Size in bytes when known.
    pub size_bytes: Option<u64>,
    /// Host-local path once the executor materialized the artifact
    /// outside the sandbox.
    pub local_path: Option<String>,
    /// In-memory bytes for small artifacts; `None` when the artifact is
    /// only on disk.
    pub bytes: Option<Vec<u8>>,
}

/// The executor contract (ADR-0022 decision 1). `submit` is the external
/// side effect of the submission flow: the task manager persists the
/// `queued` row and appends the `sandbox.task_state_changed` audit event
/// BEFORE calling it (Run iron rule, AGENTS.md §4).
#[async_trait]
pub trait SandboxExecutor: Send + Sync {
    /// Stable executor id for diagnostics (e.g. `"cube"`).
    fn id(&self) -> &str;

    /// Runtimes this executor declares — drives `sandbox_list_runtimes`
    /// and the runtime-in-list submission check (spec 5.7.1-6a).
    fn runtimes(&self) -> Vec<SandboxRuntimeInfo>;

    /// Accepts a task under a kernel-minted id. Once this returns `Ok`
    /// the task must be visible to [`SandboxExecutor::poll`].
    async fn submit(&self, task_id: &str, spec: &SandboxTaskSpec) -> Result<(), SandboxError>;

    /// Current state, output totals and usage; `TaskNotFound` for
    /// unknown ids.
    async fn poll(&self, task_id: &str) -> Result<SandboxTaskSnapshot, SandboxError>;

    /// Requests cancellation; the terminal `cancelled` state is confirmed
    /// by a subsequent [`SandboxExecutor::poll`]. Cancelling an already
    /// terminal task is a no-op success.
    async fn cancel(&self, task_id: &str) -> Result<(), SandboxError>;

    /// Incremental output since the given byte cursors. A cursor beyond
    /// the current total is a caller-side bookkeeping error.
    async fn fetch_output(
        &self,
        task_id: &str,
        stdout_from: u64,
        stderr_from: u64,
    ) -> Result<(SandboxOutputChunk, SandboxOutputChunk), SandboxError>;

    /// Fetches the task artifact. Retry-safe while the sandbox retains
    /// it; a failure surfaces as `succeeded + artifact_retryable` at the
    /// manager layer (ADR-0022: artifact return failure keeps the task
    /// succeeded and the artifact retryable).
    async fn fetch_artifact(&self, task_id: &str) -> Result<SandboxArtifact, SandboxError>;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::sandbox::fake::{FakeExecutorCall, FakeSandboxExecutor};
    use crate::sandbox::types::{SandboxLimits, SandboxTaskType};

    fn spec(runtime: &str) -> SandboxTaskSpec {
        SandboxTaskSpec {
            task_type: SandboxTaskType::RunCode,
            runtime: runtime.to_string(),
            source: "print('hi')".to_string(),
            authorized_dirs: vec!["C:\\proj\\alpha".to_string()],
            limits: SandboxLimits::default(),
        }
    }

    #[tokio::test]
    async fn trait_object_dispatches_all_five_operations() {
        let fake = Arc::new(FakeSandboxExecutor::new());
        let exec: Arc<dyn SandboxExecutor> = fake.clone();

        assert_eq!(exec.id(), "fake");
        assert!(exec.runtimes().iter().any(|r| r.id == "python"));

        exec.submit("t-1", &spec("python")).await.unwrap();
        let first = exec.poll("t-1").await.unwrap();
        assert_eq!(first.status, SandboxTaskStatus::Running);
        let last = exec.poll("t-1").await.unwrap();
        assert_eq!(last.status, SandboxTaskStatus::Succeeded);
        assert_eq!(last.exit_info.as_deref(), Some("exit 0"));
        assert_eq!(last.duration_ms, Some(1_234));

        let (stdout, stderr) = exec.fetch_output("t-1", 0, 0).await.unwrap();
        assert_eq!(stdout.text, "hello\n");
        assert_eq!(stderr.text, "");

        let artifact = exec.fetch_artifact("t-1").await.unwrap();
        assert_eq!(artifact.name, "out.txt");

        exec.cancel("t-1").await.unwrap();

        assert_eq!(
            fake.calls(),
            vec![
                FakeExecutorCall::Submit {
                    task_id: "t-1".to_string(),
                    runtime: "python".to_string(),
                },
                FakeExecutorCall::Poll {
                    task_id: "t-1".to_string(),
                },
                FakeExecutorCall::Poll {
                    task_id: "t-1".to_string(),
                },
                FakeExecutorCall::FetchOutput {
                    task_id: "t-1".to_string(),
                    stdout_from: 0,
                    stderr_from: 0,
                },
                FakeExecutorCall::FetchArtifact {
                    task_id: "t-1".to_string(),
                },
                FakeExecutorCall::Cancel {
                    task_id: "t-1".to_string(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn fetch_output_cursor_protocol_returns_deltas() {
        let fake = FakeSandboxExecutor::new();
        fake.set_outputs("t-1", "hello world", "warn");
        // Task state is created by submit; script outputs post-submit too.
        fake.submit("t-1", &spec("python")).await.unwrap();

        let (full, err_full) = fake.fetch_output("t-1", 0, 0).await.unwrap();
        assert_eq!(full.start, 0);
        assert_eq!(full.text, "hello world");
        assert_eq!(full.total, 11);
        assert_eq!(err_full.text, "warn");

        let (mid, _) = fake.fetch_output("t-1", 6, 4).await.unwrap();
        assert_eq!(mid.start, 6);
        assert_eq!(mid.text, "world");
        assert_eq!(mid.total, 11);

        let (none, no_err) = fake.fetch_output("t-1", 11, 4).await.unwrap();
        assert_eq!(none.text, "");
        assert_eq!(none.start, 11);
        assert_eq!(none.total, 11);
        assert_eq!(no_err.text, "");

        let beyond = fake.fetch_output("t-1", 12, 0).await;
        assert!(matches!(beyond, Err(SandboxError::Internal(_))));
    }

    #[tokio::test]
    async fn poll_reports_live_totals_from_output_buffers() {
        let fake = FakeSandboxExecutor::new();
        fake.submit("t-1", &spec("python")).await.unwrap();
        fake.set_outputs("t-1", "abcd", "ef");

        let snap = fake.poll("t-1").await.unwrap();
        assert_eq!(snap.status, SandboxTaskStatus::Running);
        assert_eq!(snap.stdout_total, 4);
        assert_eq!(snap.stderr_total, 2);
    }

    #[tokio::test]
    async fn cancel_switches_poll_to_cancelled_and_is_noop_when_terminal() {
        let fake = FakeSandboxExecutor::new();
        fake.submit("t-1", &spec("python")).await.unwrap();

        fake.cancel("t-1").await.unwrap();
        let snap = fake.poll("t-1").await.unwrap();
        assert_eq!(snap.status, SandboxTaskStatus::Cancelled);

        // Cancelling an already terminal task stays a no-op success.
        fake.cancel("t-1").await.unwrap();
        let again = fake.poll("t-1").await.unwrap();
        assert_eq!(again.status, SandboxTaskStatus::Cancelled);
    }

    #[tokio::test]
    async fn unknown_task_operations_return_task_not_found() {
        let fake = FakeSandboxExecutor::new();
        assert!(matches!(
            fake.poll("ghost").await,
            Err(SandboxError::TaskNotFound(_))
        ));
        assert!(matches!(
            fake.cancel("ghost").await,
            Err(SandboxError::TaskNotFound(_))
        ));
        assert!(matches!(
            fake.fetch_output("ghost", 0, 0).await,
            Err(SandboxError::TaskNotFound(_))
        ));
        assert!(matches!(
            fake.fetch_artifact("ghost").await,
            Err(SandboxError::TaskNotFound(_))
        ));
    }

    #[tokio::test]
    async fn snapshot_helpers_cover_the_lifecycle() {
        let queued = SandboxTaskSnapshot::queued();
        assert_eq!(queued.status, SandboxTaskStatus::Queued);
        assert_eq!(queued.stdout_total, 0);

        let running = SandboxTaskSnapshot::running();
        assert_eq!(running.status, SandboxTaskStatus::Running);
        assert!(!running.artifact_available);

        let terminal = SandboxTaskSnapshot::terminal(
            SandboxTaskStatus::TimedOut,
            Some("timeout after 120s".to_string()),
            Some(120_000),
        );
        assert_eq!(terminal.status, SandboxTaskStatus::TimedOut);
        assert_eq!(terminal.exit_info.as_deref(), Some("timeout after 120s"));
        assert_eq!(terminal.duration_ms, Some(120_000));
        assert_eq!(terminal.usage, None);
    }
}
