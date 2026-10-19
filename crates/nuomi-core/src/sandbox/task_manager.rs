//! Sandbox task lifecycle manager (ADR-0022 task 7.3).
//!
//! Single-writer discipline for state transitions: every status change is
//! audited as a `sandbox.task_state_changed` event (aggregate
//! `sandbox_task`/`<task_id>`, payload = task id / status / output
//! cursors / usage snapshot) and persisted in the same transaction as the
//! row update, BEFORE any external side effect (executor submit / cancel /
//! artifact fetch) — mirroring the Run state-machine iron rule
//! (AGENTS.md §4).
//!
//! The manager is a deterministic driver: every method takes a
//! `&Connection` and one `step()` advances one poll cycle. Driving loops
//! (poll-until-terminal with backoff) belong to the plugin shell
//! (task 7.5); `run_to_completion` is the bounded convenience driver.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rusqlite::Connection;
use uuid::Uuid;

use crate::domain::now_ms;
use crate::harness::{Event, EventBus};
use crate::sandbox::executor::{SandboxExecutor, SandboxOutputChunk, SandboxTaskSnapshot};
use crate::sandbox::types::{
    SandboxError, SandboxResourceUsage, SandboxRuntimeInfo, SandboxTaskSpec, SandboxTaskStatus,
};
use crate::store::repos::sandbox_tasks::{self, SandboxTaskRecord};
use crate::store::repos::{events, work_directories, workspaces};
use crate::store::StoreError;

/// Audit aggregate type for sandbox tasks.
pub const AGGREGATE_TYPE: &str = "sandbox_task";
/// Audit event kind for every state transition.
pub const EVENT_KIND: &str = "sandbox.task_state_changed";

/// Outcome of the submission pre-checks (task 7.3): the resolved active
/// workspace and the mounted-directory allowlist the spec was validated
/// against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmitCheck {
    pub workspace_id: String,
    /// Mounted work directories of the active workspace — the only paths
    /// `authorized_dirs` may reference (ADR-0021 convergence).
    pub allowed_dirs: Vec<String>,
}

/// Drives the sandbox task lifecycle against one executor.
pub struct SandboxTaskManager {
    executor: Arc<dyn SandboxExecutor>,
    /// Live `sandbox.<task_id>` channel for output deltas and state
    /// progress (ADR-0002 multi-channel pattern). `None` = audit-only.
    bus: Mutex<Option<EventBus>>,
    /// Plugin enablement: submissions are rejected while disabled
    /// (`sandbox.unavailable`); reads/cancels keep working.
    enabled: AtomicBool,
}

impl SandboxTaskManager {
    pub fn new(executor: Arc<dyn SandboxExecutor>) -> Self {
        Self {
            executor,
            bus: Mutex::new(None),
            enabled: AtomicBool::new(true),
        }
    }

    /// Attaches the kernel bus for live `sandbox.<task_id>` pushes.
    pub fn with_bus(self, bus: EventBus) -> Self {
        self.set_bus(bus);
        self
    }

    /// Attaches the kernel bus post-construction (the plugin shell wires
    /// the bus during `init`, task 7.5).
    pub fn set_bus(&self, bus: EventBus) {
        *self.lock_bus() = Some(bus);
    }

    /// Whether a live bus is attached.
    pub fn has_bus(&self) -> bool {
        self.lock_bus().is_some()
    }

    fn lock_bus(&self) -> MutexGuard<'_, Option<EventBus>> {
        self.bus.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Flips plugin enablement (task 7.5 wires start/stop to this).
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Runtimes declared by the underlying executor (drives
    /// `sandbox_list_runtimes`, task 7.6).
    pub fn runtimes(&self) -> Vec<SandboxRuntimeInfo> {
        self.executor.runtimes()
    }

    /// Submission pre-checks (task 7.3): plugin enabled, limits valid,
    /// runtime declared, and `authorized_dirs ⊆` the active workspace's
    /// mounted work directories (kernel-enforced convergence,
    /// ADR-0022 decision 3 — the executor's own file-system boundary is
    /// the second line of defense, not the only one).
    pub fn submit_check(
        &self,
        conn: &Connection,
        spec: &SandboxTaskSpec,
    ) -> Result<SubmitCheck, SandboxError> {
        if !self.is_enabled() {
            return Err(SandboxError::Unavailable(
                "sandbox plugin is disabled".to_string(),
            ));
        }
        spec.limits.validate()?;
        let workspace = workspaces::find_active(conn)
            .map_err(store_err)?
            .ok_or_else(|| {
                SandboxError::Unavailable(
                    "no active workspace: the sandbox resolves authorization against it"
                        .to_string(),
                )
            })?;
        if !self
            .executor
            .runtimes()
            .iter()
            .any(|runtime| runtime.id == spec.runtime)
        {
            return Err(SandboxError::RuntimeUnknown(format!(
                "runtime '{}' is not declared by executor '{}'",
                spec.runtime,
                self.executor.id()
            )));
        }
        let allowed_dirs: Vec<String> = work_directories::list_by_workspace(conn, &workspace.id)
            .map_err(store_err)?
            .into_iter()
            .filter(|dir| dir.mounted)
            .map(|dir| dir.abs_path)
            .collect();
        for dir in &spec.authorized_dirs {
            if !allowed_dirs.contains(dir) {
                return Err(SandboxError::DirNotAuthorized(format!(
                    "directory is not a mounted work directory of the active workspace: {dir}"
                )));
            }
        }
        Ok(SubmitCheck {
            workspace_id: workspace.id,
            allowed_dirs,
        })
    }

    /// Full submission: pre-check → insert the `queued` row → audit the
    /// queued transition → `executor.submit` (the external side effect,
    /// strictly after the audit). An executor-side rejection persists and
    /// audits a `failed` task and returns that record (the audit trail
    /// stays complete); pre-check failures return `Err` with nothing
    /// persisted. Driving starts via [`SandboxTaskManager::step`].
    pub async fn submit(
        &self,
        conn: &Connection,
        spec: &SandboxTaskSpec,
    ) -> Result<SandboxTaskRecord, SandboxError> {
        self.submit_check(conn, spec)?;
        let record = SandboxTaskRecord {
            id: Uuid::now_v7().to_string(),
            task_type: spec.task_type,
            runtime: spec.runtime.clone(),
            status: SandboxTaskStatus::Queued,
            source: spec.source.clone(),
            authorized_dirs: spec.authorized_dirs.clone(),
            limits: spec.limits.clone(),
            stdout: String::new(),
            stderr: String::new(),
            artifact_ref: None,
            artifact_retryable: false,
            exit_info: None,
            duration_ms: None,
            usage: None,
            created_at: now_ms(),
            finished_at: None,
        };
        sandbox_tasks::insert(conn, &record).map_err(store_err)?;
        self.audit_transition(
            conn,
            &record.id,
            &Transition {
                status: SandboxTaskStatus::Queued,
                stdout_cursor: 0,
                stderr_cursor: 0,
                usage: None,
                exit_info: None,
                duration_ms: None,
            },
        )?;

        if let Err(err) = self.executor.submit(&record.id, spec).await {
            let exit_info = format!("submit failed: {err}");
            self.audit_transition(
                conn,
                &record.id,
                &Transition {
                    status: SandboxTaskStatus::Failed,
                    stdout_cursor: 0,
                    stderr_cursor: 0,
                    usage: None,
                    exit_info: Some(&exit_info),
                    duration_ms: None,
                },
            )?;
            sandbox_tasks::mark_terminal(
                conn,
                &record.id,
                SandboxTaskStatus::Failed,
                Some(&exit_info),
                None,
                now_ms(),
            )
            .map_err(store_err)?;
            let failed = self.get_task(conn, &record.id)?;
            self.publish_progress(&failed, "", "");
            return Ok(failed);
        }
        self.get_task(conn, &record.id)
    }

    /// Advances one poll cycle: executor.poll (read) → pull output deltas
    /// (read) → persist outputs → apply the status transition (audit event
    /// and row update in one transaction) → external side effects
    /// (artifact fetch, watchdog cancel) → publish `sandbox.<task_id>`
    /// progress. Stepping a terminal task is an idempotent no-op.
    pub async fn step(
        &self,
        conn: &Connection,
        task_id: &str,
    ) -> Result<SandboxTaskRecord, SandboxError> {
        let mut record = self.get_task(conn, task_id)?;
        if record.status.is_terminal() {
            return Ok(record);
        }
        let snapshot = self.executor.poll(task_id).await?;

        // Manager-side watchdog (defense in depth): the executor enforces
        // timeout_secs itself; this catches executors gone silent.
        let limit_ms = (record.limits.timeout_secs.saturating_mul(1_000)) as i64;
        let overdue = now_ms().saturating_sub(record.created_at) > limit_ms;
        if overdue && !snapshot.status.is_terminal() {
            return self.watchdog_time_out(conn, record, &snapshot).await;
        }

        match snapshot.status {
            SandboxTaskStatus::Queued => Ok(record),
            SandboxTaskStatus::Running => {
                let (stdout_delta, stderr_delta) = self.fetch_deltas(&record).await?;
                self.apply_outputs(conn, &mut record, &stdout_delta, &stderr_delta)?;
                if record.status == SandboxTaskStatus::Queued {
                    self.persist_transition(
                        conn,
                        &record.id,
                        &Transition {
                            status: SandboxTaskStatus::Running,
                            stdout_cursor: record.stdout.len() as u64,
                            stderr_cursor: record.stderr.len() as u64,
                            usage: snapshot.usage.as_ref(),
                            exit_info: None,
                            duration_ms: None,
                        },
                    )?;
                    record.status = SandboxTaskStatus::Running;
                }
                self.publish_progress(&record, &stdout_delta, &stderr_delta);
                Ok(record)
            }
            status => {
                debug_assert!(status.is_terminal());
                let (stdout_delta, stderr_delta) = self.fetch_deltas(&record).await?;
                self.apply_outputs(conn, &mut record, &stdout_delta, &stderr_delta)?;
                self.persist_transition(
                    conn,
                    &record.id,
                    &Transition {
                        status,
                        stdout_cursor: record.stdout.len() as u64,
                        stderr_cursor: record.stderr.len() as u64,
                        usage: snapshot.usage.as_ref(),
                        exit_info: snapshot.exit_info.as_deref(),
                        duration_ms: snapshot.duration_ms,
                    },
                )?;
                // Return the persisted truth (finished_at et al.), not a
                // second now_ms() guess.
                let mut record = self.get_task(conn, task_id)?;
                self.resolve_artifact(conn, &mut record, &snapshot).await?;
                self.publish_progress(&record, &stdout_delta, &stderr_delta);
                Ok(record)
            }
        }
    }

    /// Bounded convenience driver: steps until terminal (tests and short
    /// tasks). Production driving loops live in the plugin shell (7.5).
    pub async fn run_to_completion(
        &self,
        conn: &Connection,
        task_id: &str,
    ) -> Result<SandboxTaskRecord, SandboxError> {
        const MAX_STEPS: usize = 256;
        for _ in 0..MAX_STEPS {
            let record = self.step(conn, task_id).await?;
            if record.status.is_terminal() {
                return Ok(record);
            }
        }
        Err(SandboxError::Internal(
            "task did not reach a terminal state within the step budget".to_string(),
        ))
    }

    /// Requests cancellation. This is NOT a state transition: the
    /// `cancelled` status lands on the next [`SandboxTaskManager::step`]
    /// that observes the executor's cancelled snapshot, keeping every
    /// transition inside the audited single-writer path. Cancelling a
    /// terminal or missing task is `TaskNotFound` / no-op respectively.
    pub async fn cancel(&self, conn: &Connection, task_id: &str) -> Result<(), SandboxError> {
        let record = self.get_task(conn, task_id)?;
        if record.status.is_terminal() {
            return Ok(());
        }
        self.executor.cancel(task_id).await
    }

    /// Reads one task (`sandbox_get_task`, task 7.6).
    pub fn get_task(
        &self,
        conn: &Connection,
        task_id: &str,
    ) -> Result<SandboxTaskRecord, SandboxError> {
        sandbox_tasks::find_by_id(conn, task_id)
            .map_err(store_err)?
            .ok_or_else(|| SandboxError::TaskNotFound(task_id.to_string()))
    }

    /// Lists recent tasks (`sandbox_list_tasks`, task 7.6).
    pub fn list_recent(
        &self,
        conn: &Connection,
        limit: u32,
    ) -> Result<Vec<SandboxTaskRecord>, SandboxError> {
        sandbox_tasks::list_recent(conn, limit).map_err(store_err)
    }

    /// Retries the artifact return for a terminal task whose earlier
    /// fetch failed (`artifact_retryable`): success records the location
    /// and clears the flag; failure keeps it retryable (the sandbox
    /// retains the artifact — ADR-0022). Tasks without a pending retry
    /// return unchanged (idempotent).
    pub async fn retry_artifact(
        &self,
        conn: &Connection,
        task_id: &str,
    ) -> Result<SandboxTaskRecord, SandboxError> {
        let mut record = self.get_task(conn, task_id)?;
        if !record.artifact_retryable {
            return Ok(record);
        }
        match self.executor.fetch_artifact(&record.id).await {
            Ok(artifact) => {
                let reference = artifact
                    .local_path
                    .clone()
                    .or_else(|| Some(artifact.name.clone()));
                sandbox_tasks::set_artifact(conn, &record.id, reference.as_deref(), false)
                    .map_err(store_err)?;
                record.artifact_ref = reference;
                record.artifact_retryable = false;
            }
            Err(err) => {
                tracing::warn!(
                    task_id = %record.id,
                    error = %err,
                    "sandbox artifact retry failed; stays retryable"
                );
            }
        }
        self.publish_progress(&record, "", "");
        Ok(record)
    }

    // ---- internals --------------------------------------------------------

    /// Fetches the output deltas since the row's stored cursors
    /// (`stdout.len()` / `stderr.len()`). A chunk that does not start at
    /// the requested cursor is an executor protocol violation.
    async fn fetch_deltas(
        &self,
        record: &SandboxTaskRecord,
    ) -> Result<(String, String), SandboxError> {
        let (stdout, stderr) = self
            .executor
            .fetch_output(
                &record.id,
                record.stdout.len() as u64,
                record.stderr.len() as u64,
            )
            .await?;
        verify_chunk(&stdout, record.stdout.len() as u64, "stdout")?;
        verify_chunk(&stderr, record.stderr.len() as u64, "stderr")?;
        Ok((stdout.text, stderr.text))
    }

    /// Persists non-empty deltas onto the row (keeping produced output on
    /// abort paths — spec: 超时/资源超限中止并保留已产生输出).
    fn apply_outputs(
        &self,
        conn: &Connection,
        record: &mut SandboxTaskRecord,
        stdout_delta: &str,
        stderr_delta: &str,
    ) -> Result<(), SandboxError> {
        if stdout_delta.is_empty() && stderr_delta.is_empty() {
            return Ok(());
        }
        sandbox_tasks::append_output(conn, &record.id, stdout_delta, stderr_delta)
            .map_err(store_err)?;
        record.stdout.push_str(stdout_delta);
        record.stderr.push_str(stderr_delta);
        Ok(())
    }

    /// Appends one `sandbox.task_state_changed` event (no row update).
    fn audit_transition(
        &self,
        conn: &Connection,
        task_id: &str,
        transition: &Transition<'_>,
    ) -> Result<(), SandboxError> {
        let payload = transition_payload(task_id, transition);
        events::append(
            conn,
            AGGREGATE_TYPE,
            task_id,
            EVENT_KIND,
            &payload,
            now_ms(),
        )
        .map(|_| ())
        .map_err(store_err)
    }

    /// Audited transition: `sandbox.task_state_changed` event + row update
    /// in ONE transaction (audit first), so the log and the row can never
    /// disagree. Callers must invoke external side effects only after
    /// this returns.
    fn persist_transition(
        &self,
        conn: &Connection,
        task_id: &str,
        transition: &Transition<'_>,
    ) -> Result<(), SandboxError> {
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| store_err(e.into()))?;
        let payload = transition_payload(task_id, transition);
        events::append(&tx, AGGREGATE_TYPE, task_id, EVENT_KIND, &payload, now_ms())
            .map(|_| ())
            .map_err(store_err)?;
        if transition.status == SandboxTaskStatus::Running {
            sandbox_tasks::mark_running(&tx, task_id).map_err(store_err)?;
        } else {
            sandbox_tasks::mark_terminal(
                &tx,
                task_id,
                transition.status,
                transition.exit_info,
                transition.duration_ms,
                now_ms(),
            )
            .map_err(store_err)?;
            // Terminal usage lands on the row in the same audited
            // transaction (spec 5.7.1-5); executors that never report
            // usage leave the column NULL.
            if let Some(usage) = transition.usage {
                sandbox_tasks::set_usage(&tx, task_id, usage).map_err(store_err)?;
            }
        }
        tx.commit().map_err(|e| store_err(e.into()))
    }

    /// Artifact resolution after a terminal transition: success records
    /// the location; failure keeps the task `succeeded` with
    /// `artifact_retryable` (the sandbox retains the artifact for a
    /// retry — ADR-0022).
    async fn resolve_artifact(
        &self,
        conn: &Connection,
        record: &mut SandboxTaskRecord,
        snapshot: &SandboxTaskSnapshot,
    ) -> Result<(), SandboxError> {
        if !snapshot.artifact_available {
            return Ok(());
        }
        match self.executor.fetch_artifact(&record.id).await {
            Ok(artifact) => {
                let reference = artifact
                    .local_path
                    .clone()
                    .or_else(|| Some(artifact.name.clone()));
                sandbox_tasks::set_artifact(conn, &record.id, reference.as_deref(), false)
                    .map_err(store_err)?;
                record.artifact_ref = reference;
                record.artifact_retryable = false;
            }
            Err(err) => {
                tracing::warn!(
                    task_id = %record.id,
                    error = %err,
                    "sandbox artifact fetch failed; marked retryable"
                );
                sandbox_tasks::set_artifact(conn, &record.id, None, true).map_err(store_err)?;
                record.artifact_ref = None;
                record.artifact_retryable = true;
            }
        }
        Ok(())
    }

    /// Watchdog path for overdue non-terminal tasks: keep the produced
    /// output, transition to `timed_out` (audited), THEN best-effort
    /// executor cancel as the external side effect.
    async fn watchdog_time_out(
        &self,
        conn: &Connection,
        mut record: SandboxTaskRecord,
        _snapshot: &SandboxTaskSnapshot,
    ) -> Result<SandboxTaskRecord, SandboxError> {
        let (stdout_delta, stderr_delta) = match self.fetch_deltas(&record).await {
            Ok(deltas) => deltas,
            Err(err) => {
                tracing::warn!(
                    task_id = %record.id,
                    error = %err,
                    "sandbox watchdog: final output fetch failed"
                );
                (String::new(), String::new())
            }
        };
        self.apply_outputs(conn, &mut record, &stdout_delta, &stderr_delta)?;
        let duration_ms = now_ms().saturating_sub(record.created_at) as u64;
        let exit_info = "manager watchdog: exceeded timeout_secs".to_string();
        self.persist_transition(
            conn,
            &record.id,
            &Transition {
                status: SandboxTaskStatus::TimedOut,
                stdout_cursor: record.stdout.len() as u64,
                stderr_cursor: record.stderr.len() as u64,
                usage: None,
                exit_info: Some(&exit_info),
                duration_ms: Some(duration_ms),
            },
        )?;
        // Persisted truth (finished_at stamped inside the transition).
        let record = self.get_task(conn, &record.id)?;
        if let Err(err) = self.executor.cancel(&record.id).await {
            tracing::warn!(
                task_id = %record.id,
                error = %err,
                "sandbox watchdog: executor cancel failed"
            );
        }
        self.publish_progress(&record, &stdout_delta, &stderr_delta);
        Ok(record)
    }

    /// Publishes progress on the `sandbox.<task_id>` channel (output
    /// increments per ADR-0022 §4; payload also carries the current
    /// status so the frontend can render transitions).
    fn publish_progress(&self, record: &SandboxTaskRecord, stdout_delta: &str, stderr_delta: &str) {
        let Some(bus) = self.lock_bus().clone() else {
            return;
        };
        let payload = serde_json::json!({
            "task_id": record.id,
            "status": record.status.as_str(),
            "stdout": stdout_delta,
            "stderr": stderr_delta,
            "stdout_total": record.stdout.len(),
            "stderr_total": record.stderr.len(),
            "exit_info": record.exit_info,
            "duration_ms": record.duration_ms,
            "artifact_retryable": record.artifact_retryable,
        });
        bus.publish(Event::new(format!("sandbox.{}", record.id), payload));
    }
}

fn verify_chunk(
    chunk: &SandboxOutputChunk,
    expected_start: u64,
    stream: &str,
) -> Result<(), SandboxError> {
    if chunk.start != expected_start {
        return Err(SandboxError::Internal(format!(
            "executor {stream} chunk starts at {} but cursor was {expected_start}",
            chunk.start
        )));
    }
    Ok(())
}

/// One audited state transition's event payload pieces: task status, the
/// output cursors as of the transition, and the executor-reported usage /
/// exit / duration snapshot (ADR-0022 §4).
struct Transition<'a> {
    status: SandboxTaskStatus,
    stdout_cursor: u64,
    stderr_cursor: u64,
    usage: Option<&'a SandboxResourceUsage>,
    exit_info: Option<&'a str>,
    duration_ms: Option<u64>,
}

fn transition_payload(task_id: &str, transition: &Transition<'_>) -> serde_json::Value {
    serde_json::json!({
        "task_id": task_id,
        "status": transition.status.as_str(),
        "stdout_cursor": transition.stdout_cursor,
        "stderr_cursor": transition.stderr_cursor,
        "usage": transition.usage,
        "exit_info": transition.exit_info,
        "duration_ms": transition.duration_ms,
    })
}

fn store_err(err: StoreError) -> SandboxError {
    SandboxError::Internal(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use tempfile::TempDir;

    use crate::sandbox::executor::SandboxArtifact;
    use crate::sandbox::fake::{FakeExecutorCall, FakeSandboxExecutor};
    use crate::sandbox::types::{SandboxLimits, SandboxTaskType};
    use crate::store::repos::workspaces::WorkspaceEntry;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::store::migrations::run(&conn).unwrap();
        conn
    }

    /// Active workspace with `C:\proj\alpha` mounted and `C:\proj\beta`
    /// registered but unmounted.
    fn seed_active_workspace(conn: &Connection) {
        workspaces::insert(
            conn,
            &WorkspaceEntry {
                id: "w1".to_string(),
                root_path: "C:\\proj".to_string(),
                color_tag: "paper-yellow".to_string(),
                created_at: 1,
                is_active: true,
                is_pinned: false,
            },
        )
        .unwrap();
        work_directories::add(conn, "w1", "C:\\proj\\alpha", "alpha").unwrap();
        let beta = work_directories::add(conn, "w1", "C:\\proj\\beta", "beta").unwrap();
        work_directories::set_mounted(conn, &beta.id, false).unwrap();
    }

    fn spec_for(runtime: &str, dirs: &[&str]) -> SandboxTaskSpec {
        SandboxTaskSpec {
            task_type: SandboxTaskType::RunCode,
            runtime: runtime.to_string(),
            source: "print('hi')".to_string(),
            authorized_dirs: dirs.iter().map(|d| d.to_string()).collect(),
            limits: SandboxLimits::default(),
        }
    }

    fn audited_statuses(conn: &Connection, task_id: &str) -> Vec<String> {
        events::list_by_aggregate(conn, AGGREGATE_TYPE, task_id, None)
            .unwrap()
            .iter()
            .map(|e| {
                e.payload
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or_default()
                    .to_string()
            })
            .collect()
    }

    // ---- submission checks (7.3-2) ---------------------------------------

    #[test]
    fn submit_check_rejects_when_plugin_disabled() {
        let conn = db();
        seed_active_workspace(&conn);
        let manager = SandboxTaskManager::new(Arc::new(FakeSandboxExecutor::new()));
        assert!(manager.is_enabled());
        manager.set_enabled(false);
        assert!(!manager.is_enabled());
        let err = manager
            .submit_check(&conn, &spec_for("python", &[]))
            .unwrap_err();
        assert!(matches!(err, SandboxError::Unavailable(_)));
    }

    #[test]
    fn submit_check_rejects_invalid_limits() {
        let conn = db();
        seed_active_workspace(&conn);
        let manager = SandboxTaskManager::new(Arc::new(FakeSandboxExecutor::new()));
        let mut spec = spec_for("python", &[]);
        spec.limits.timeout_secs = 0;
        assert!(matches!(
            manager.submit_check(&conn, &spec),
            Err(SandboxError::LimitInvalid(_))
        ));
    }

    #[test]
    fn submit_check_requires_an_active_workspace() {
        let conn = db();
        let manager = SandboxTaskManager::new(Arc::new(FakeSandboxExecutor::new()));
        let err = manager
            .submit_check(&conn, &spec_for("python", &[]))
            .unwrap_err();
        assert!(matches!(err, SandboxError::Unavailable(_)));
    }

    #[test]
    fn submit_check_rejects_runtime_outside_declared_list() {
        let conn = db();
        seed_active_workspace(&conn);
        let manager = SandboxTaskManager::new(Arc::new(FakeSandboxExecutor::new()));
        let err = manager
            .submit_check(&conn, &spec_for("ruby", &[]))
            .unwrap_err();
        assert!(matches!(err, SandboxError::RuntimeUnknown(_)));
    }

    #[test]
    fn submit_check_converges_authorized_dirs_to_mounted_workspace_dirs() {
        let conn = db();
        seed_active_workspace(&conn);
        let manager = SandboxTaskManager::new(Arc::new(FakeSandboxExecutor::new()));

        let check = manager
            .submit_check(&conn, &spec_for("python", &["C:\\proj\\alpha"]))
            .unwrap();
        assert_eq!(check.workspace_id, "w1");
        assert_eq!(check.allowed_dirs, vec!["C:\\proj\\alpha".to_string()]);

        // 未挂载的工作目录不算授权集
        let unmounted = manager
            .submit_check(&conn, &spec_for("python", &["C:\\proj\\beta"]))
            .unwrap_err();
        assert!(matches!(unmounted, SandboxError::DirNotAuthorized(_)));

        let foreign = manager
            .submit_check(&conn, &spec_for("python", &["D:\\evil"]))
            .unwrap_err();
        assert!(matches!(foreign, SandboxError::DirNotAuthorized(_)));
    }

    // ---- submission + iron rule ------------------------------------------

    #[tokio::test]
    async fn submit_creates_queued_task_with_audit_event() {
        let conn = db();
        seed_active_workspace(&conn);
        let fake = Arc::new(FakeSandboxExecutor::new());
        let manager = SandboxTaskManager::new(fake.clone());

        let record = manager
            .submit(&conn, &spec_for("python", &["C:\\proj\\alpha"]))
            .await
            .unwrap();
        assert!(Uuid::parse_str(&record.id).is_ok());
        assert_eq!(record.status, SandboxTaskStatus::Queued);
        assert!(record.created_at > 0);
        assert!(record.finished_at.is_none());

        assert_eq!(audited_statuses(&conn, &record.id), vec!["queued"]);
        let events = events::list_by_aggregate(&conn, AGGREGATE_TYPE, &record.id, None).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EVENT_KIND);
        assert_eq!(events[0].aggregate_type, AGGREGATE_TYPE);
        assert_eq!(events[0].payload["task_id"], record.id);
        assert_eq!(events[0].payload["status"], "queued");
        assert_eq!(events[0].payload["stdout_cursor"], 0);

        assert!(matches!(
            fake.calls().as_slice(),
            [FakeExecutorCall::Submit { .. }]
        ));
    }

    #[tokio::test]
    async fn terminal_usage_lands_on_the_row_and_survives_queries() {
        let conn = db();
        seed_active_workspace(&conn);
        let fake = Arc::new(FakeSandboxExecutor::new());
        let manager = SandboxTaskManager::new(fake.clone());
        let record = manager
            .submit(&conn, &spec_for("python", &["C:\\proj\\alpha"]))
            .await
            .unwrap();

        let mut terminal = SandboxTaskSnapshot::terminal(
            SandboxTaskStatus::Succeeded,
            Some("exit 0".to_string()),
            Some(1_234),
        );
        terminal.usage = Some(SandboxResourceUsage {
            cpu_time_ms: Some(120),
            peak_memory_mb: Some(64),
        });
        fake.set_poll_script(&record.id, vec![terminal]);

        let done = manager.run_to_completion(&conn, &record.id).await.unwrap();
        let expected = Some(SandboxResourceUsage {
            cpu_time_ms: Some(120),
            peak_memory_mb: Some(64),
        });
        assert_eq!(done.status, SandboxTaskStatus::Succeeded);
        assert_eq!(done.usage, expected);

        // Row-level queries return the usage too (spec 5.7.1-5a).
        assert_eq!(manager.get_task(&conn, &record.id).unwrap().usage, expected);
        let listed = manager.list_recent(&conn, 10).unwrap();
        assert_eq!(listed.first().unwrap().usage, expected);
    }

    #[tokio::test]
    async fn terminal_without_reported_usage_keeps_column_null() {
        let conn = db();
        seed_active_workspace(&conn);
        let manager = SandboxTaskManager::new(Arc::new(FakeSandboxExecutor::new()));
        let record = manager
            .submit(&conn, &spec_for("python", &["C:\\proj\\alpha"]))
            .await
            .unwrap();

        // Default script: running → succeeded without usage.
        let done = manager.run_to_completion(&conn, &record.id).await.unwrap();
        assert_eq!(done.status, SandboxTaskStatus::Succeeded);
        assert_eq!(done.usage, None);
        assert_eq!(manager.get_task(&conn, &record.id).unwrap().usage, None);
    }

    /// Iron-rule probe: opens its OWN connection and checks whether the
    /// `queued` audit event is already committed when `submit` runs.
    struct ProbeExecutor {
        db_path: String,
        queued_event_visible_at_submit: Mutex<Option<bool>>,
    }

    #[async_trait]
    impl SandboxExecutor for ProbeExecutor {
        fn id(&self) -> &str {
            "probe"
        }

        fn runtimes(&self) -> Vec<SandboxRuntimeInfo> {
            vec![SandboxRuntimeInfo {
                id: "python".to_string(),
                name: "Python (probe)".to_string(),
                isolation: "probe executor".to_string(),
            }]
        }

        async fn submit(&self, task_id: &str, _spec: &SandboxTaskSpec) -> Result<(), SandboxError> {
            let visible = crate::store::Db::open(&self.db_path)
                .ok()
                .and_then(|db| events::list_by_aggregate(&db.0, AGGREGATE_TYPE, task_id, None).ok())
                .map(|evs| {
                    evs.iter().any(|e| {
                        e.kind == EVENT_KIND
                            && e.payload.get("status").and_then(|s| s.as_str()) == Some("queued")
                    })
                })
                .unwrap_or(false);
            *self.queued_event_visible_at_submit.lock().unwrap() = Some(visible);
            Ok(())
        }

        async fn poll(&self, _task_id: &str) -> Result<SandboxTaskSnapshot, SandboxError> {
            Ok(SandboxTaskSnapshot::terminal(
                SandboxTaskStatus::Succeeded,
                Some("exit 0".to_string()),
                Some(5),
            ))
        }

        async fn cancel(&self, _task_id: &str) -> Result<(), SandboxError> {
            Ok(())
        }

        async fn fetch_output(
            &self,
            _task_id: &str,
            stdout_from: u64,
            stderr_from: u64,
        ) -> Result<(SandboxOutputChunk, SandboxOutputChunk), SandboxError> {
            Ok((
                SandboxOutputChunk::empty_at(stdout_from),
                SandboxOutputChunk::empty_at(stderr_from),
            ))
        }

        async fn fetch_artifact(&self, _task_id: &str) -> Result<SandboxArtifact, SandboxError> {
            Err(SandboxError::Internal("probe has no artifact".to_string()))
        }
    }

    #[tokio::test]
    async fn audit_event_lands_before_executor_submit_side_effect() {
        // File-backed DB: a second connection observes committed events
        // (WAL), which is exactly what the probe asserts mid-submit.
        let dir: TempDir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nuomi.db").to_string_lossy().to_string();
        let conn = {
            let db = crate::store::Db::open(&path).unwrap();
            crate::store::migrations::run(&db.0).unwrap();
            db.0
        };
        seed_active_workspace(&conn);
        let probe = Arc::new(ProbeExecutor {
            db_path: path,
            queued_event_visible_at_submit: Mutex::new(None),
        });
        let manager = SandboxTaskManager::new(probe.clone());

        let record = manager
            .submit(&conn, &spec_for("python", &["C:\\proj\\alpha"]))
            .await
            .unwrap();
        assert_eq!(record.status, SandboxTaskStatus::Queued);
        assert_eq!(
            *probe.queued_event_visible_at_submit.lock().unwrap(),
            Some(true),
            "queued 审计事件必须先于 executor.submit 副作用落库"
        );
    }

    // ---- lifecycle paths ---------------------------------------------------

    #[tokio::test]
    async fn happy_path_reaches_succeeded_with_output_and_artifact() {
        let conn = db();
        seed_active_workspace(&conn);
        let fake = Arc::new(FakeSandboxExecutor::new());
        let manager = SandboxTaskManager::new(fake.clone());

        let record = manager
            .submit(&conn, &spec_for("python", &[]))
            .await
            .unwrap();
        let done = manager.run_to_completion(&conn, &record.id).await.unwrap();

        assert_eq!(done.status, SandboxTaskStatus::Succeeded);
        assert_eq!(done.stdout, "hello\n");
        assert_eq!(done.exit_info.as_deref(), Some("exit 0"));
        assert_eq!(done.duration_ms, Some(1_234));
        assert_eq!(done.artifact_ref.as_deref(), Some("out.txt"));
        assert!(!done.artifact_retryable);
        assert!(done.finished_at.unwrap() >= done.created_at);

        assert_eq!(
            audited_statuses(&conn, &record.id),
            vec!["queued", "running", "succeeded"]
        );
        let events = events::list_by_aggregate(&conn, AGGREGATE_TYPE, &record.id, None).unwrap();
        let terminal = events.last().unwrap();
        assert_eq!(terminal.payload["stdout_cursor"], 6, "终态事件携带最终游标");

        let calls = fake.calls();
        assert!(calls
            .iter()
            .any(|c| matches!(c, FakeExecutorCall::FetchOutput { stdout_from: 0, .. })));
        assert!(calls
            .iter()
            .any(|c| matches!(c, FakeExecutorCall::FetchArtifact { .. })));
    }

    #[tokio::test]
    async fn executor_submit_failure_persists_failed_task() {
        let conn = db();
        seed_active_workspace(&conn);
        let fake = Arc::new(FakeSandboxExecutor::new());
        let manager = SandboxTaskManager::new(fake.clone());
        fake.fail_next_submit(SandboxError::Unavailable("executor down".to_string()));

        let record = manager
            .submit(&conn, &spec_for("python", &[]))
            .await
            .unwrap();
        assert_eq!(record.status, SandboxTaskStatus::Failed);
        let exit = record.exit_info.unwrap();
        assert!(exit.contains("submit failed"));
        assert!(exit.contains("executor down"));
        assert_eq!(
            audited_statuses(&conn, &record.id),
            vec!["queued", "failed"]
        );
    }

    #[tokio::test]
    async fn cancel_lands_cancelled_on_next_step() {
        let conn = db();
        seed_active_workspace(&conn);
        let fake = Arc::new(FakeSandboxExecutor::new());
        let manager = SandboxTaskManager::new(fake.clone());

        let record = manager
            .submit(&conn, &spec_for("python", &[]))
            .await
            .unwrap();
        manager.cancel(&conn, &record.id).await.unwrap();

        // cancel 本身不是状态迁移：终态由下一次 step 观察落库
        assert_eq!(
            manager.get_task(&conn, &record.id).unwrap().status,
            SandboxTaskStatus::Queued
        );
        let done = manager.step(&conn, &record.id).await.unwrap();
        assert_eq!(done.status, SandboxTaskStatus::Cancelled);
        assert_eq!(
            audited_statuses(&conn, &record.id),
            vec!["queued", "cancelled"]
        );
        assert!(fake
            .calls()
            .iter()
            .any(|c| matches!(c, FakeExecutorCall::Cancel { .. })));
    }

    #[tokio::test]
    async fn watchdog_marks_overdue_task_timed_out_and_keeps_output() {
        let conn = db();
        seed_active_workspace(&conn);
        let fake = Arc::new(FakeSandboxExecutor::new());
        let manager = SandboxTaskManager::new(fake.clone());

        // 回溯 created_at：1s 预算的任务已跑 10s（绕过 manager.submit）
        let mut overdue = spec_record("t-overdue", 10_000);
        overdue.limits = SandboxLimits {
            timeout_secs: 1,
            ..SandboxLimits::default()
        };
        sandbox_tasks::insert(&conn, &overdue).unwrap();
        fake.set_outputs("t-overdue", "partial output", "");
        fake.submit("t-overdue", &spec_for("python", &[]))
            .await
            .unwrap();

        let done = manager.step(&conn, "t-overdue").await.unwrap();
        assert_eq!(done.status, SandboxTaskStatus::TimedOut);
        assert!(done.exit_info.unwrap().contains("watchdog"));
        assert!(done.duration_ms.unwrap() >= 10_000);
        assert_eq!(done.stdout, "partial output", "超时中止保留已产生输出");
        assert_eq!(audited_statuses(&conn, "t-overdue"), vec!["timed_out"]);
        assert!(fake
            .calls()
            .iter()
            .any(|c| matches!(c, FakeExecutorCall::Cancel { .. })));
    }

    #[tokio::test]
    async fn artifact_fetch_failure_keeps_succeeded_and_marks_retryable() {
        let conn = db();
        seed_active_workspace(&conn);
        let fake = Arc::new(FakeSandboxExecutor::new());
        let manager = SandboxTaskManager::new(fake.clone());

        let record = manager
            .submit(&conn, &spec_for("python", &[]))
            .await
            .unwrap();
        fake.set_artifact(
            &record.id,
            Err(SandboxError::Internal("transfer failed".to_string())),
        );
        let done = manager.run_to_completion(&conn, &record.id).await.unwrap();

        assert_eq!(done.status, SandboxTaskStatus::Succeeded);
        assert!(done.artifact_retryable);
        assert!(done.artifact_ref.is_none());

        // 重试成功：清 retryable、落定位；对非 retryable 任务幂等
        fake.set_artifact(
            &record.id,
            Ok(crate::sandbox::executor::SandboxArtifact {
                name: "app.exe".to_string(),
                size_bytes: Some(4),
                local_path: Some("C:\\nuomi\\.nuomi\\sandbox\\app.exe".to_string()),
                bytes: None,
            }),
        );
        let retried = manager.retry_artifact(&conn, &record.id).await.unwrap();
        assert!(!retried.artifact_retryable);
        assert_eq!(
            retried.artifact_ref.as_deref(),
            Some("C:\\nuomi\\.nuomi\\sandbox\\app.exe")
        );
        // 再次重试（已无待重试）原样返回
        let again = manager.retry_artifact(&conn, &record.id).await.unwrap();
        assert!(!again.artifact_retryable);

        // 重试仍失败：保持 retryable
        fake.set_artifact(
            &record.id,
            Err(SandboxError::Internal("still failing".to_string())),
        );
        let other = manager.submit(&conn, &spec_for("node", &[])).await.unwrap();
        fake.set_artifact(
            &other.id,
            Err(SandboxError::Internal("transfer failed".to_string())),
        );
        manager.run_to_completion(&conn, &other.id).await.unwrap();
        fake.set_artifact(
            &other.id,
            Err(SandboxError::Internal("still failing".to_string())),
        );
        let still = manager.retry_artifact(&conn, &other.id).await.unwrap();
        assert!(still.artifact_retryable);
        assert!(still.artifact_ref.is_none());
    }

    #[tokio::test]
    async fn output_deltas_publish_on_sandbox_task_channel() {
        let conn = db();
        seed_active_workspace(&conn);
        let bus = EventBus::new(64);
        let mut rx = bus.subscribe();
        let manager = SandboxTaskManager::new(Arc::new(FakeSandboxExecutor::new())).with_bus(bus);

        let record = manager
            .submit(&conn, &spec_for("python", &[]))
            .await
            .unwrap();
        let done = manager.run_to_completion(&conn, &record.id).await.unwrap();
        assert_eq!(done.status, SandboxTaskStatus::Succeeded);

        let mut saw_delta = false;
        let mut saw_terminal = false;
        let mut count = 0;
        while let Ok(event) = rx.try_recv() {
            assert_eq!(event.topic, format!("sandbox.{}", record.id));
            if event.payload["stdout"].as_str() == Some("hello\n") {
                saw_delta = true;
            }
            if event.payload["status"].as_str() == Some("succeeded") {
                saw_terminal = true;
            }
            count += 1;
        }
        assert!(count >= 2, "running 与终态各推一次");
        assert!(saw_delta, "输出增量必须推送到 sandbox.<task_id> 通道");
        assert!(saw_terminal);
    }

    #[tokio::test]
    async fn step_on_terminal_task_is_idempotent() {
        let conn = db();
        seed_active_workspace(&conn);
        let manager = SandboxTaskManager::new(Arc::new(FakeSandboxExecutor::new()));
        let record = manager
            .submit(&conn, &spec_for("python", &[]))
            .await
            .unwrap();
        let done = manager.run_to_completion(&conn, &record.id).await.unwrap();
        let again = manager.step(&conn, &record.id).await.unwrap();
        assert_eq!(again, done);
        assert_eq!(audited_statuses(&conn, &record.id).len(), 3);
    }

    #[tokio::test]
    async fn missing_task_operations_return_task_not_found() {
        let conn = db();
        let manager = SandboxTaskManager::new(Arc::new(FakeSandboxExecutor::new()));
        assert!(matches!(
            manager.get_task(&conn, "ghost"),
            Err(SandboxError::TaskNotFound(_))
        ));
        assert!(matches!(
            manager.step(&conn, "ghost").await,
            Err(SandboxError::TaskNotFound(_))
        ));
        assert!(matches!(
            manager.cancel(&conn, "ghost").await,
            Err(SandboxError::TaskNotFound(_))
        ));
        assert!(manager.list_recent(&conn, 10).unwrap().is_empty());
    }

    /// Builds a queued row for direct repo insertion (watchdog test).
    fn spec_record(id: &str, age_ms: i64) -> SandboxTaskRecord {
        SandboxTaskRecord {
            id: id.to_string(),
            task_type: SandboxTaskType::RunCode,
            runtime: "python".to_string(),
            status: SandboxTaskStatus::Queued,
            source: "print('hi')".to_string(),
            authorized_dirs: vec![],
            limits: SandboxLimits::default(),
            stdout: String::new(),
            stderr: String::new(),
            artifact_ref: None,
            artifact_retryable: false,
            exit_info: None,
            duration_ms: None,
            usage: None,
            created_at: now_ms() - age_ms,
            finished_at: None,
        }
    }
}
