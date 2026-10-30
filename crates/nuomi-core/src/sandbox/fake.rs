//! Deterministic fake executor for tests (mirrors `providers/fake.rs`).
//!
//! Replays a scripted poll sequence per task and implements the real
//! output-cursor protocol over in-memory buffers, so task-manager and
//! plugin tests exercise the exact wire behavior. Every trait call is
//! recorded for order assertions (audit-before-side-effect checks).
//!
//! Scripting helpers (`set_outputs` / `set_poll_script` / `set_artifact`)
//! lazily create the task slot, so they work before or after `submit`.

use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::executor::{SandboxArtifact, SandboxExecutor, SandboxOutputChunk, SandboxTaskSnapshot};
use super::types::{SandboxError, SandboxRuntimeInfo, SandboxTaskSpec, SandboxTaskStatus};

/// One recorded executor call, in order (for audit-order assertions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeExecutorCall {
    Submit {
        task_id: String,
        runtime: String,
    },
    Poll {
        task_id: String,
    },
    Cancel {
        task_id: String,
    },
    FetchOutput {
        task_id: String,
        stdout_from: u64,
        stderr_from: u64,
    },
    FetchArtifact {
        task_id: String,
    },
}

/// A fake task's scripted state.
struct FakeTask {
    /// Replayed on successive polls; the last entry repeats.
    poll_script: VecDeque<SandboxTaskSnapshot>,
    stdout: String,
    stderr: String,
    artifact: Option<Result<SandboxArtifact, SandboxError>>,
    cancelled: bool,
    /// Last status the fake reported; terminal ⇒ cancel becomes a no-op.
    last_status: Option<SandboxTaskStatus>,
}

impl FakeTask {
    fn new(script: &[SandboxTaskSnapshot]) -> Self {
        Self {
            poll_script: script.iter().cloned().collect(),
            stdout: "hello\n".to_string(),
            stderr: String::new(),
            artifact: Some(Ok(SandboxArtifact {
                name: "out.txt".to_string(),
                size_bytes: Some(6),
                local_path: None,
                bytes: Some(b"hello\n".to_vec()),
            })),
            cancelled: false,
            last_status: None,
        }
    }

    fn task_mut<'a>(inner: &'a mut FakeInner, task_id: &str) -> Result<&'a mut Self, SandboxError> {
        inner
            .tasks
            .get_mut(task_id)
            .ok_or_else(|| SandboxError::TaskNotFound(task_id.to_string()))
    }
}

struct FakeInner {
    calls: Vec<FakeExecutorCall>,
    next_submit_error: Option<SandboxError>,
    tasks: HashMap<String, FakeTask>,
}

/// Scriptable [`SandboxExecutor`] fake for tests.
pub struct FakeSandboxExecutor {
    runtimes: Vec<SandboxRuntimeInfo>,
    default_script: Vec<SandboxTaskSnapshot>,
    inner: Mutex<FakeInner>,
}

impl FakeSandboxExecutor {
    /// Fake with `python` / `node` / `rust` runtimes and the default
    /// running → succeeded poll script. The declared list mirrors
    /// `CubeSandboxExecutor::default_runtimes()` so tests asserting the
    /// default runtime surface stay aligned with the real executor.
    pub fn new() -> Self {
        Self {
            runtimes: vec![
                SandboxRuntimeInfo {
                    id: "python".to_string(),
                    name: "Python 3 (fake)".to_string(),
                    isolation: "fake in-memory executor (no isolation)".to_string(),
                },
                SandboxRuntimeInfo {
                    id: "node".to_string(),
                    name: "Node.js (fake)".to_string(),
                    isolation: "fake in-memory executor (no isolation)".to_string(),
                },
                SandboxRuntimeInfo {
                    id: "rust".to_string(),
                    name: "Rust (fake)".to_string(),
                    isolation: "fake in-memory executor (no isolation)".to_string(),
                },
            ],
            default_script: vec![
                SandboxTaskSnapshot::running(),
                SandboxTaskSnapshot::terminal(
                    SandboxTaskStatus::Succeeded,
                    Some("exit 0".to_string()),
                    Some(1_234),
                ),
            ],
            inner: Mutex::new(FakeInner {
                calls: Vec::new(),
                next_submit_error: None,
                tasks: HashMap::new(),
            }),
        }
    }

    /// Replaces the declared runtime list.
    pub fn with_runtimes(mut self, runtimes: Vec<SandboxRuntimeInfo>) -> Self {
        self.runtimes = runtimes;
        self
    }

    /// Replaces the default poll script applied to newly submitted tasks.
    pub fn with_default_script(mut self, script: Vec<SandboxTaskSnapshot>) -> Self {
        self.default_script = script;
        self
    }

    /// Scripts the next submit to fail (submission-side failure).
    pub fn fail_next_submit(&self, err: SandboxError) {
        self.lock_inner().next_submit_error = Some(err);
    }

    /// Overwrites a task's poll script (lazily creates the task slot).
    pub fn set_poll_script(&self, task_id: &str, script: Vec<SandboxTaskSnapshot>) {
        let mut inner = self.lock_inner();
        let task = inner
            .tasks
            .entry(task_id.to_string())
            .or_insert_with(|| FakeTask::new(&self.default_script));
        task.poll_script = script.into_iter().collect();
    }

    /// Sets a task's in-memory output streams (lazily creates the slot).
    pub fn set_outputs(&self, task_id: &str, stdout: &str, stderr: &str) {
        let mut inner = self.lock_inner();
        let task = inner
            .tasks
            .entry(task_id.to_string())
            .or_insert_with(|| FakeTask::new(&self.default_script));
        task.stdout = stdout.to_string();
        task.stderr = stderr.to_string();
    }

    /// Scripts the artifact fetch result; `Err` simulates a failed
    /// artifact return (lazily creates the slot).
    pub fn set_artifact(&self, task_id: &str, result: Result<SandboxArtifact, SandboxError>) {
        let mut inner = self.lock_inner();
        let task = inner
            .tasks
            .entry(task_id.to_string())
            .or_insert_with(|| FakeTask::new(&self.default_script));
        task.artifact = Some(result);
    }

    /// Recorded calls so far, in order.
    pub fn calls(&self) -> Vec<FakeExecutorCall> {
        self.lock_inner().calls.clone()
    }

    fn lock_inner(&self) -> MutexGuard<'_, FakeInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for FakeSandboxExecutor {
    fn default() -> Self {
        Self::new()
    }
}

/// Slices an in-memory stream at the cursor (the real fetch protocol).
fn slice_stream(stream: &str, from: u64) -> Result<SandboxOutputChunk, SandboxError> {
    let total = stream.len() as u64;
    if from > total {
        return Err(SandboxError::Internal(format!(
            "cursor {from} beyond stream total {total}"
        )));
    }
    let text = stream.get(from as usize..).unwrap_or_default();
    Ok(SandboxOutputChunk {
        start: from,
        text: text.to_string(),
        total,
        truncated: false,
    })
}

#[async_trait]
impl SandboxExecutor for FakeSandboxExecutor {
    fn id(&self) -> &str {
        "fake"
    }

    fn runtimes(&self) -> Vec<SandboxRuntimeInfo> {
        self.runtimes.clone()
    }

    async fn submit(&self, task_id: &str, spec: &SandboxTaskSpec) -> Result<(), SandboxError> {
        let mut inner = self.lock_inner();
        inner.calls.push(FakeExecutorCall::Submit {
            task_id: task_id.to_string(),
            runtime: spec.runtime.clone(),
        });
        if let Some(err) = inner.next_submit_error.take() {
            return Err(err);
        }
        // entry-or-insert keeps pre-submit scripting (set_outputs etc.)
        // from being clobbered by the default task template.
        inner
            .tasks
            .entry(task_id.to_string())
            .or_insert_with(|| FakeTask::new(&self.default_script));
        Ok(())
    }

    async fn poll(&self, task_id: &str) -> Result<SandboxTaskSnapshot, SandboxError> {
        let mut inner = self.lock_inner();
        inner.calls.push(FakeExecutorCall::Poll {
            task_id: task_id.to_string(),
        });
        let task = FakeTask::task_mut(&mut inner, task_id)?;
        let base = if task.cancelled {
            Some(SandboxTaskSnapshot::terminal(
                SandboxTaskStatus::Cancelled,
                Some("cancelled by user".to_string()),
                None,
            ))
        } else if task.poll_script.len() > 1 {
            task.poll_script.pop_front()
        } else {
            task.poll_script.front().cloned()
        };
        let mut snap = base.unwrap_or_else(|| {
            SandboxTaskSnapshot::terminal(
                SandboxTaskStatus::Failed,
                Some("fake: empty poll script".to_string()),
                None,
            )
        });
        // Keep the fake self-consistent: totals mirror the buffers and
        // artifact availability mirrors the scripted artifact (a scripted
        // Err still reads "available" — the fetch itself then fails,
        // modeling return-transfer failures).
        snap.stdout_total = task.stdout.len() as u64;
        snap.stderr_total = task.stderr.len() as u64;
        snap.artifact_available = task.artifact.is_some();
        task.last_status = Some(snap.status);
        Ok(snap)
    }

    async fn cancel(&self, task_id: &str) -> Result<(), SandboxError> {
        let mut inner = self.lock_inner();
        inner.calls.push(FakeExecutorCall::Cancel {
            task_id: task_id.to_string(),
        });
        let task = FakeTask::task_mut(&mut inner, task_id)?;
        if !task.last_status.is_some_and(|s| s.is_terminal()) {
            task.cancelled = true;
        }
        Ok(())
    }

    async fn fetch_output(
        &self,
        task_id: &str,
        stdout_from: u64,
        stderr_from: u64,
    ) -> Result<(SandboxOutputChunk, SandboxOutputChunk), SandboxError> {
        let mut inner = self.lock_inner();
        inner.calls.push(FakeExecutorCall::FetchOutput {
            task_id: task_id.to_string(),
            stdout_from,
            stderr_from,
        });
        let task = FakeTask::task_mut(&mut inner, task_id)?;
        let stdout = slice_stream(&task.stdout, stdout_from)?;
        let stderr = slice_stream(&task.stderr, stderr_from)?;
        Ok((stdout, stderr))
    }

    async fn fetch_artifact(&self, task_id: &str) -> Result<SandboxArtifact, SandboxError> {
        let mut inner = self.lock_inner();
        inner.calls.push(FakeExecutorCall::FetchArtifact {
            task_id: task_id.to_string(),
        });
        let task = FakeTask::task_mut(&mut inner, task_id)?;
        match &task.artifact {
            Some(Ok(artifact)) => Ok(artifact.clone()),
            Some(Err(err)) => Err(err.clone()),
            None => Err(SandboxError::Internal(format!(
                "fake: task {task_id} has no artifact"
            ))),
        }
    }
}
