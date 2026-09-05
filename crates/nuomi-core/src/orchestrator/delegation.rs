//! Non-blocking delegation broker (codeg delegation/broker pattern, research:
//! `docs/research/codeg.md` §2 "非阻塞委托 Broker").
//!
//! - [`DelegationBroker::start`] returns a task id immediately and runs the
//!   task in a background `tokio::spawn`; the parent loop is never blocked.
//! - [`DelegationBroker::wait`] long-polls on a `watch` channel and is woken
//!   on completion — no busy polling.
//! - [`DelegationBroker::cancel`] cancels all descendants recursively
//!   (children first), then the task itself; already-terminal tasks are
//!   untouched.
//! - Completion and cancellation race under one map lock: first writer wins
//!   and terminal states are never overwritten (a late cancel arriving after
//!   completion is a no-op; a late completion after cancel is discarded).

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{mpsc, watch, Mutex};

use crate::domain::new_id;

/// Identifier of a delegation task (project-wide uuid-v7 string ids).
pub type TaskId = String;

/// Buffered progress events per running task before the receiver is taken.
const EVENT_BUFFER: usize = 64;

/// Work handed to a [`DelegateExecutor`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationTask {
    pub title: String,
    pub payload: serde_json::Value,
}

/// Final result of a successful delegation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationOutcome {
    pub summary: String,
    pub output: serde_json::Value,
}

/// Progress events streamed by an executor while its task runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DelegationEvent {
    Log(String),
}

/// Actual execution body injected by the caller (CLI adapters, providers,
/// test fakes). The broker never blocks on it: it runs behind a `tokio::spawn`
/// and is dropped if the task is cancelled first.
#[async_trait]
pub trait DelegateExecutor: Send + Sync + 'static {
    async fn execute(
        &self,
        task: DelegationTask,
        events: mpsc::Sender<DelegationEvent>,
    ) -> Result<DelegationOutcome, DelegationError>;
}

/// Lifecycle states of a delegation task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TaskStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskStatus {
    /// Terminal states keep their result record forever.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TaskStatus::Succeeded | TaskStatus::Failed | TaskStatus::Cancelled
        )
    }

    /// Canonical log/IPC text form (`snake_case`).
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::Running => "running",
            TaskStatus::Succeeded => "succeeded",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }
}

impl std::fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Errors produced by the delegation broker and its executors.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DelegationError {
    #[error(
        "delegation depth limit exceeded: ancestor chain {chain} + 1 exceeds max depth {max_depth}"
    )]
    DepthLimitExceeded { chain: u32, max_depth: u32 },

    #[error("unknown delegation task: {0}")]
    UnknownTask(String),

    #[error("delegation task cancelled")]
    Cancelled,

    #[error("delegation executor failed: {0}")]
    Executor(String),
}

struct TaskEntry {
    parent: Option<TaskId>,
    children: Vec<TaskId>,
    /// Live status mirror; `wait` and the spawned wrapper subscribe to it.
    status: watch::Sender<TaskStatus>,
    /// Kept alive so the channel is never closed: `watch::Sender::send`
    /// neither stores nor notifies once every receiver is dropped, which
    /// would lose a cancel racing the task's first poll.
    status_rx: watch::Receiver<TaskStatus>,
    /// Completion cache, written exactly once when the task reaches a
    /// terminal state (first writer wins the cancel/complete race).
    result: Option<Result<DelegationOutcome, DelegationError>>,
    /// Progress stream, handed out once via [`DelegationBroker::take_events`].
    events: Option<mpsc::Receiver<DelegationEvent>>,
}

struct Inner {
    executor: Arc<dyn DelegateExecutor>,
    max_depth: u32,
    tasks: Mutex<HashMap<TaskId, TaskEntry>>,
}

/// Broker for background delegation tasks with depth-limited nesting,
/// long-poll waiting and cascading cancellation.
#[derive(Clone)]
pub struct DelegationBroker {
    inner: Arc<Inner>,
}

impl DelegationBroker {
    pub fn new(executor: Arc<dyn DelegateExecutor>, max_depth: u32) -> Self {
        Self {
            inner: Arc::new(Inner {
                executor,
                max_depth,
                tasks: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn max_depth(&self) -> u32 {
        self.inner.max_depth
    }

    /// Spawns `task` in the background and returns its id immediately.
    ///
    /// When `parent` is set, the ancestor chain is validated first: if
    /// `chain + 1 > max_depth` the delegation is rejected (codeg default
    /// `max_depth = 1`, i.e. a delegated task may not delegate further).
    pub async fn start(
        &self,
        parent: Option<TaskId>,
        task: DelegationTask,
    ) -> Result<TaskId, DelegationError> {
        let task_id: TaskId = new_id();
        let mut tasks = self.inner.tasks.lock().await;
        if let Some(parent_id) = &parent {
            let chain = Self::ancestor_chain(&tasks, parent_id)?;
            if chain + 1 > self.inner.max_depth {
                return Err(DelegationError::DepthLimitExceeded {
                    chain,
                    max_depth: self.inner.max_depth,
                });
            }
            if let Some(parent_entry) = tasks.get_mut(parent_id) {
                parent_entry.children.push(task_id.clone());
            }
        }
        let (status, status_rx) = watch::channel(TaskStatus::Pending);
        let (events_tx, events_rx) = mpsc::channel(EVENT_BUFFER);
        tasks.insert(
            task_id.clone(),
            TaskEntry {
                parent: parent.clone(),
                children: Vec::new(),
                status,
                status_rx,
                result: None,
                events: Some(events_rx),
            },
        );
        drop(tasks);
        tokio::spawn(Self::run(
            self.inner.clone(),
            task_id.clone(),
            task,
            events_tx,
        ));
        Ok(task_id)
    }

    /// Length of the ancestor chain above `parent` (parent itself included).
    fn ancestor_chain(
        tasks: &HashMap<TaskId, TaskEntry>,
        parent: &TaskId,
    ) -> Result<u32, DelegationError> {
        let mut chain = 0u32;
        let mut cursor = Some(parent.clone());
        while let Some(id) = cursor {
            let entry = tasks
                .get(&id)
                .ok_or_else(|| DelegationError::UnknownTask(id.clone()))?;
            chain += 1;
            cursor = entry.parent.clone();
        }
        Ok(chain)
    }

    /// Background body: runs the executor, racing its future against the
    /// cancellation signal carried by the task's own status watch.
    async fn run(
        inner: Arc<Inner>,
        task_id: TaskId,
        task: DelegationTask,
        events_tx: mpsc::Sender<DelegationEvent>,
    ) {
        let mut rx = {
            let tasks = inner.tasks.lock().await;
            match tasks.get(&task_id) {
                Some(entry) => {
                    // Cancelled before the task got its first poll: never run.
                    if entry.status.borrow().is_terminal() {
                        return;
                    }
                    let _ = entry.status.send(TaskStatus::Running);
                    entry.status.subscribe()
                }
                None => return,
            }
        };
        let executor = inner.executor.clone();
        tokio::select! {
            outcome = executor.execute(task, events_tx) => {
                Self::finish(&inner, &task_id, outcome).await;
            }
            _ = wait_for_cancelled(&mut rx) => {
                // `cancel` already recorded the terminal Cancelled state;
                // dropping the executor future here is the interruption.
            }
        }
    }

    /// Records the completion; a no-op when cancellation won the race.
    async fn finish(
        inner: &Inner,
        task_id: &TaskId,
        outcome: Result<DelegationOutcome, DelegationError>,
    ) {
        let mut tasks = inner.tasks.lock().await;
        let Some(entry) = tasks.get_mut(task_id) else {
            return;
        };
        if entry.status.borrow().is_terminal() {
            return; // late completion after cancel is discarded
        }
        let status = if outcome.is_ok() {
            TaskStatus::Succeeded
        } else {
            TaskStatus::Failed
        };
        let _ = entry.status.send(status);
        entry.result = Some(outcome);
    }

    /// Long-polls until the task reaches a terminal state; woken by the
    /// status watch, never busy-polling. Returns the completion cache.
    pub async fn wait(&self, task_id: &TaskId) -> Result<DelegationOutcome, DelegationError> {
        let mut rx = {
            let tasks = self.inner.tasks.lock().await;
            tasks
                .get(task_id)
                .ok_or_else(|| DelegationError::UnknownTask(task_id.clone()))?
                .status
                .subscribe()
        };
        rx.wait_for(|s| s.is_terminal()).await.map_err(|_| {
            DelegationError::Executor("broker dropped before completion".to_string())
        })?;
        let tasks = self.inner.tasks.lock().await;
        match tasks.get(task_id).and_then(|e| e.result.clone()) {
            Some(result) => result,
            None => Err(DelegationError::Executor(
                "terminal task missing completion record".to_string(),
            )),
        }
    }

    /// Current lifecycle state; `Err(UnknownTask)` for unknown ids.
    pub async fn status(&self, task_id: &TaskId) -> Result<TaskStatus, DelegationError> {
        let tasks = self.inner.tasks.lock().await;
        tasks
            .get(task_id)
            .map(|e| *e.status_rx.borrow())
            .ok_or_else(|| DelegationError::UnknownTask(task_id.clone()))
    }

    /// Cancels the task and, recursively, all of its descendants — children
    /// first, then the task itself. Already-terminal tasks are untouched, so
    /// a late cancel never overrides a completed child.
    pub async fn cancel(&self, task_id: &TaskId) -> Result<(), DelegationError> {
        let mut tasks = self.inner.tasks.lock().await;
        if !tasks.contains_key(task_id) {
            return Err(DelegationError::UnknownTask(task_id.clone()));
        }
        let mut order = Vec::new();
        collect_descendants_postorder(&tasks, task_id, &mut order);
        for id in order {
            let Some(entry) = tasks.get_mut(&id) else {
                continue;
            };
            if entry.status.borrow().is_terminal() {
                continue;
            }
            let _ = entry.status.send(TaskStatus::Cancelled);
            entry.result = Some(Err(DelegationError::Cancelled));
        }
        Ok(())
    }

    /// Hands out the task's progress stream receiver (once).
    pub async fn take_events(&self, task_id: &TaskId) -> Option<mpsc::Receiver<DelegationEvent>> {
        let mut tasks = self.inner.tasks.lock().await;
        tasks.get_mut(task_id).and_then(|e| e.events.take())
    }
}

/// Descendants of `root` in post-order (deepest children first, `root` last),
/// matching the cancel order "descendants, then self".
fn collect_descendants_postorder(
    tasks: &HashMap<TaskId, TaskEntry>,
    root: &TaskId,
    out: &mut Vec<TaskId>,
) {
    if let Some(entry) = tasks.get(root) {
        for child in &entry.children {
            collect_descendants_postorder(tasks, child, out);
        }
        out.push(root.clone());
    }
}

/// Resolves as soon as the watched status is `Cancelled`.
async fn wait_for_cancelled(rx: &mut watch::Receiver<TaskStatus>) {
    loop {
        if *rx.borrow_and_update() == TaskStatus::Cancelled {
            return;
        }
        if rx.changed().await.is_err() {
            // Broker dropped; nothing will ever cancel this task again.
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap as TestMap;

    /// Fake executor gated by per-title [`tokio::sync::Notify`] handles so
    /// tests control completion timing without real sleeping. A task whose
    /// title has no gate completes immediately; payload `"fail": true` makes
    /// it fail.
    struct GatedExecutor {
        gates: TestMap<String, Arc<tokio::sync::Notify>>,
    }

    impl GatedExecutor {
        fn gated(titles: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                gates: titles
                    .iter()
                    .map(|t| ((*t).to_string(), Arc::new(tokio::sync::Notify::new())))
                    .collect(),
            })
        }

        fn release(&self, title: &str) {
            if let Some(gate) = self.gates.get(title) {
                gate.notify_one();
            }
        }
    }

    #[async_trait]
    impl DelegateExecutor for GatedExecutor {
        async fn execute(
            &self,
            task: DelegationTask,
            events: mpsc::Sender<DelegationEvent>,
        ) -> Result<DelegationOutcome, DelegationError> {
            let _ = events
                .send(DelegationEvent::Log("working".to_string()))
                .await;
            if let Some(gate) = self.gates.get(&task.title) {
                gate.notified().await;
            }
            let fail = task
                .payload
                .get("fail")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if fail {
                return Err(DelegationError::Executor("boom".to_string()));
            }
            Ok(DelegationOutcome {
                summary: "landed".to_string(),
                output: serde_json::json!({ "ok": true }),
            })
        }
    }

    fn broker(titles: &[&str], max_depth: u32) -> (DelegationBroker, Arc<GatedExecutor>) {
        let executor = GatedExecutor::gated(titles);
        (DelegationBroker::new(executor.clone(), max_depth), executor)
    }

    fn task(title: &str, fail: bool) -> DelegationTask {
        DelegationTask {
            title: title.to_string(),
            payload: serde_json::json!({ "fail": fail }),
        }
    }

    #[tokio::test]
    async fn start_returns_immediately_and_long_poll_wakes_on_completion() {
        let (b, ex) = broker(&["root"], 1);
        let id = b.start(None, task("root", false)).await.unwrap();
        // start returned while the executor is still parked on its gate
        let status = b.status(&id).await.unwrap();
        assert!(matches!(status, TaskStatus::Pending | TaskStatus::Running));
        // long-poll is woken by the watch when the executor completes
        let waiter = {
            let b = b.clone();
            let id = id.clone();
            tokio::spawn(async move { b.wait(&id).await })
        };
        ex.release("root");
        let outcome = waiter.await.unwrap().unwrap();
        assert_eq!(outcome.summary, "landed");
        assert_eq!(b.status(&id).await.unwrap(), TaskStatus::Succeeded);
    }

    #[tokio::test]
    async fn depth_limit_rejects_over_nested_delegation() {
        let (b, _ex) = broker(&[], 2);
        let root = b.start(None, task("root", false)).await.unwrap();
        let child = b
            .start(Some(root.clone()), task("child", false))
            .await
            .unwrap();
        // chain 2 + 1 > 2 → rejected
        let err = b
            .start(Some(child.clone()), task("grandchild", false))
            .await
            .unwrap_err();
        assert_eq!(
            err,
            DelegationError::DepthLimitExceeded {
                chain: 2,
                max_depth: 2
            }
        );
        // unknown parent is rejected too
        assert!(matches!(
            b.start(Some("missing".to_string()), task("x", false)).await,
            Err(DelegationError::UnknownTask(_))
        ));
    }

    #[tokio::test]
    async fn cancel_cascades_to_grandchildren() {
        let (b, _ex) = broker(&["root", "child", "grandchild"], 3);
        let root = b.start(None, task("root", false)).await.unwrap();
        let child = b
            .start(Some(root.clone()), task("child", false))
            .await
            .unwrap();
        let grandchild = b
            .start(Some(child.clone()), task("grandchild", false))
            .await
            .unwrap();
        b.cancel(&root).await.unwrap();
        for id in [&root, &child, &grandchild] {
            assert_eq!(b.status(id).await.unwrap(), TaskStatus::Cancelled);
            assert_eq!(b.wait(id).await.unwrap_err(), DelegationError::Cancelled);
        }
    }

    #[tokio::test]
    async fn cancel_spares_completed_children() {
        // only root and grandchild are gated; child completes immediately
        let (b, _ex) = broker(&["root", "grandchild"], 3);
        let root = b.start(None, task("root", false)).await.unwrap();
        let child = b
            .start(Some(root.clone()), task("child", false))
            .await
            .unwrap();
        assert_eq!(b.wait(&child).await.unwrap().summary, "landed");
        let grandchild = b
            .start(Some(child.clone()), task("grandchild", false))
            .await
            .unwrap();
        b.cancel(&root).await.unwrap();
        assert_eq!(b.status(&root).await.unwrap(), TaskStatus::Cancelled);
        assert_eq!(b.status(&child).await.unwrap(), TaskStatus::Succeeded);
        assert_eq!(b.status(&grandchild).await.unwrap(), TaskStatus::Cancelled);
    }

    #[tokio::test]
    async fn late_cancel_does_not_override_completion() {
        let (b, _ex) = broker(&[], 1);
        let id = b.start(None, task("root", false)).await.unwrap();
        b.wait(&id).await.unwrap();
        b.cancel(&id).await.unwrap();
        assert_eq!(b.status(&id).await.unwrap(), TaskStatus::Succeeded);
        // the stored completion record is still readable
        assert_eq!(b.wait(&id).await.unwrap().summary, "landed");
    }

    #[tokio::test]
    async fn executor_failure_marks_task_failed() {
        let (b, _ex) = broker(&[], 1);
        let id = b.start(None, task("root", true)).await.unwrap();
        let err = b.wait(&id).await.unwrap_err();
        assert_eq!(err, DelegationError::Executor("boom".to_string()));
        assert_eq!(b.status(&id).await.unwrap(), TaskStatus::Failed);
    }

    #[tokio::test]
    async fn events_are_buffered_until_taken() {
        let (b, _ex) = broker(&[], 1);
        let id = b.start(None, task("root", false)).await.unwrap();
        b.wait(&id).await.unwrap();
        let mut rx = b.take_events(&id).await.unwrap();
        assert_eq!(
            rx.recv().await,
            Some(DelegationEvent::Log("working".to_string()))
        );
        // the receiver is handed out exactly once
        assert!(b.take_events(&id).await.is_none());
    }

    #[tokio::test]
    async fn unknown_task_errors_on_every_operation() {
        let (b, _ex) = broker(&[], 1);
        let id: TaskId = "missing".to_string();
        assert!(matches!(
            b.status(&id).await,
            Err(DelegationError::UnknownTask(_))
        ));
        assert!(matches!(
            b.wait(&id).await,
            Err(DelegationError::UnknownTask(_))
        ));
        assert!(matches!(
            b.cancel(&id).await,
            Err(DelegationError::UnknownTask(_))
        ));
    }
}
