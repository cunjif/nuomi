//! Sandbox plugin shell (ADR-0022 task 7.5).
//!
//! The builtin `sandbox` plugin owns the executor wiring: `init` registers
//! the [`SandboxService`] surface (used by the IPC layer, task 7.6),
//! `start` enables submissions and spawns the background driver loop,
//! and `dispose` stops the loop and terminates in-flight tasks (cancel +
//! one observing step per task, so every teardown transition stays inside
//! the audited single-writer path).
//!
//! Executor replacement needs no kernel change: construct the plugin with
//! any [`SandboxExecutor`] (the default is [`CubeSandboxExecutor`]; tests
//! inject `FakeSandboxExecutor`).
//!
//! ## Why the driver loop is not `tokio::spawn(manager.step(...))`
//!
//! `rusqlite::Connection` is `Send` but not `Sync`, so any future that
//! holds `&Connection` across an await point (which every
//! [`SandboxTaskManager`] method does) is `!Send` and cannot be spawned
//! onto the tokio scheduler. The service therefore runs those futures via
//! [`call_with_connection`]: a `spawn_blocking` thread with its own
//! current-thread runtime `block_on`ing the non-Send future, returning a
//! Send handle the async caller awaits. The outer driver-loop future
//! itself stays Send and spawns normally.
//!
//! Disabling the plugin (service `set_enabled(false)`) only hides the
//! sandbox entry points (submissions rejected as `sandbox.unavailable`);
//! reads, cancels and already-running tasks keep working, and the rest of
//! the application is unaffected (spec 5.7.1-1a/1b).

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use async_trait::async_trait;
use rusqlite::Connection;
use tokio_util::sync::CancellationToken;

use crate::harness::{Context, HarnessError, Plugin};
use crate::sandbox::task_manager::SandboxTaskManager;
use crate::sandbox::types::{SandboxError, SandboxRuntimeInfo, SandboxTaskSpec};
use crate::store::repos::sandbox_tasks::SandboxTaskRecord;
use crate::store::Db;

/// Default driver-loop poll interval.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How many recent tasks one driver tick inspects.
const DRIVER_BATCH: u32 = 64;

/// Runs `f` with a fresh connection on a blocking thread. `f` returns a
/// future that may hold `&Connection` across awaits (the manager's
/// non-Send futures); the blocking thread's current-thread runtime
/// `block_on`s it, so the handle this function returns stays Send.
async fn call_with_connection<T, F, Fut>(db_path: &Arc<str>, f: F) -> Result<T, SandboxError>
where
    T: Send + 'static,
    F: FnOnce(Connection) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, SandboxError>> + 'static,
{
    let path = Arc::clone(db_path);
    tokio::task::spawn_blocking(move || -> Result<T, SandboxError> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| SandboxError::Internal(format!("sandbox worker runtime: {e}")))?;
        let db = Db::open(&path).map_err(|e| SandboxError::Internal(e.to_string()))?;
        crate::store::migrations::run(&db.0).map_err(|e| SandboxError::Internal(e.to_string()))?;
        rt.block_on(f(db.0))
    })
    .await
    .map_err(|e| SandboxError::Internal(format!("sandbox worker join failed: {e}")))?
}

/// Service surface registered as `"sandbox"` (consumed by the IPC layer,
/// task 7.6, and by the plugin's own tests).
#[derive(Clone)]
pub struct SandboxService {
    db_path: Arc<str>,
    manager: Arc<SandboxTaskManager>,
}

impl SandboxService {
    pub fn new(db_path: impl Into<Arc<str>>, manager: Arc<SandboxTaskManager>) -> Self {
        Self {
            db_path: db_path.into(),
            manager,
        }
    }

    /// The wrapped manager (driver wiring and tests).
    pub fn manager(&self) -> &Arc<SandboxTaskManager> {
        &self.manager
    }

    /// Flips the enablement switch: disabled hides the sandbox entry
    /// points (submissions fail `sandbox.unavailable`); reads, cancels
    /// and running tasks keep working.
    pub fn set_enabled(&self, enabled: bool) {
        self.manager.set_enabled(enabled);
    }

    pub fn is_enabled(&self) -> bool {
        self.manager.is_enabled()
    }

    /// Runtimes declared by the executor (`sandbox_list_runtimes`).
    pub fn runtimes(&self) -> Vec<SandboxRuntimeInfo> {
        self.manager.runtimes()
    }

    /// Submits a task (pre-checks → queued row → audit → executor
    /// submit). Executor-side rejection persists a `failed` task.
    pub async fn submit(&self, spec: SandboxTaskSpec) -> Result<SandboxTaskRecord, SandboxError> {
        let manager = Arc::clone(&self.manager);
        call_with_connection(&self.db_path, move |conn| async move {
            manager.submit(&conn, &spec).await
        })
        .await
    }

    /// Lists recent tasks (`sandbox_list_tasks`).
    pub async fn list_tasks(&self, limit: u32) -> Result<Vec<SandboxTaskRecord>, SandboxError> {
        let manager = Arc::clone(&self.manager);
        call_with_connection(&self.db_path, move |conn| async move {
            manager.list_recent(&conn, limit)
        })
        .await
    }

    /// Reads one task (`sandbox_get_task`).
    pub async fn get_task(&self, task_id: &str) -> Result<SandboxTaskRecord, SandboxError> {
        let manager = Arc::clone(&self.manager);
        let task_id = task_id.to_string();
        call_with_connection(&self.db_path, move |conn| async move {
            manager.get_task(&conn, &task_id)
        })
        .await
    }

    /// Requests cancellation; the `cancelled` status lands on the next
    /// observing step (audited single-writer path).
    pub async fn cancel_task(&self, task_id: &str) -> Result<(), SandboxError> {
        let manager = Arc::clone(&self.manager);
        let task_id = task_id.to_string();
        call_with_connection(&self.db_path, move |conn| async move {
            manager.cancel(&conn, &task_id).await
        })
        .await
    }

    /// Advances one task one poll cycle (manual driving / tests).
    pub async fn step_task(&self, task_id: &str) -> Result<SandboxTaskRecord, SandboxError> {
        let manager = Arc::clone(&self.manager);
        let task_id = task_id.to_string();
        call_with_connection(&self.db_path, move |conn| async move {
            manager.step(&conn, &task_id).await
        })
        .await
    }
}

/// Background driver loop state (filled by `start`, drained by `dispose`).
struct DriverHandle {
    cancel: CancellationToken,
    join: tokio::task::JoinHandle<()>,
}

/// The builtin sandbox plugin: registers the service, drives the poll
/// loop, terminates in-flight tasks on teardown.
pub struct SandboxPlugin {
    service: SandboxService,
    poll_interval: Duration,
    driver: Mutex<Option<DriverHandle>>,
}

impl SandboxPlugin {
    /// Plugin over any executor (replacement = constructing the plugin
    /// with another `SandboxExecutor` implementation).
    pub fn with_executor(
        executor: Arc<dyn crate::sandbox::executor::SandboxExecutor>,
        db_path: impl Into<Arc<str>>,
    ) -> Self {
        let manager = Arc::new(SandboxTaskManager::new(executor));
        Self::from_manager(manager, db_path)
    }

    /// Plugin over the default [`crate::sandbox::CubeSandboxExecutor`] at
    /// `base_url`.
    pub fn new(base_url: impl Into<String>, db_path: impl Into<Arc<str>>) -> Self {
        Self::with_executor(
            Arc::new(crate::sandbox::CubeSandboxExecutor::new(base_url)),
            db_path,
        )
    }

    /// Plugin over an existing manager (bus already attached).
    pub fn from_manager(manager: Arc<SandboxTaskManager>, db_path: impl Into<Arc<str>>) -> Self {
        Self {
            service: SandboxService::new(db_path, manager),
            poll_interval: DEFAULT_POLL_INTERVAL,
            driver: Mutex::new(None),
        }
    }

    /// Shortens the driver poll interval (tests).
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// The service this plugin registers (for hosts that wire manually).
    pub fn service(&self) -> &SandboxService {
        &self.service
    }

    fn take_driver(&self) -> Option<DriverHandle> {
        self.driver
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

#[async_trait]
impl Plugin for SandboxPlugin {
    fn id(&self) -> &str {
        "sandbox"
    }

    async fn init(&self, ctx: &Context) -> Result<(), HarnessError> {
        // Attach the kernel bus for live sandbox.<task_id> pushes before
        // the service goes visible.
        let manager = self.service.manager();
        manager.set_bus(ctx.bus());
        ctx.register_service(self.id(), "sandbox", Arc::new(self.service.clone()))
            .await
    }

    async fn start(&self) -> Result<(), HarnessError> {
        self.service.set_enabled(true);
        let mut slot = self.driver.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_some() {
            return Ok(()); // idempotent start
        }
        let cancel = CancellationToken::new();
        let join = tokio::spawn(drive_loop(
            Arc::clone(self.service.manager()),
            Arc::clone(self.service.db_path()),
            cancel.clone(),
            self.poll_interval,
        ));
        *slot = Some(DriverHandle { cancel, join });
        Ok(())
    }

    async fn dispose(&self) -> Result<(), HarnessError> {
        if let Some(handle) = self.take_driver() {
            handle.cancel.cancel();
            // Waits for the current tick to finish, then the loop
            // terminates in-flight tasks (cancel + observing step) and
            // exits.
            if let Err(e) = handle.join.await {
                return Err(HarnessError::PluginFailed {
                    plugin: self.id().to_string(),
                    phase: "dispose",
                    message: format!("driver loop join failed: {e}"),
                });
            }
        }
        self.service.set_enabled(false);
        Ok(())
    }
}

impl SandboxService {
    /// Database path every worker connection opens.
    pub fn db_path(&self) -> &Arc<str> {
        &self.db_path
    }
}

/// The Send driver loop: each tick steps every non-terminal recent task
/// on a blocking worker (see [`call_with_connection`]); on cancellation
/// it terminates in-flight tasks before exiting.
async fn drive_loop(
    manager: Arc<SandboxTaskManager>,
    db_path: Arc<str>,
    cancel: CancellationToken,
    interval: Duration,
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                terminate_inflight(&manager, &db_path).await;
                break;
            }
            _ = ticker.tick() => {
                step_batch(&manager, &db_path).await;
            }
        }
    }
}

/// One driver tick: advance every non-terminal recent task.
async fn step_batch(manager: &Arc<SandboxTaskManager>, db_path: &Arc<str>) {
    let m = Arc::clone(manager);
    let _ = call_with_connection(db_path, move |conn| async move {
        let tasks = m.list_recent(&conn, DRIVER_BATCH)?;
        for task in &tasks {
            if task.status.is_terminal() {
                continue;
            }
            if let Err(err) = m.step(&conn, &task.id).await {
                eprintln!("sandbox driver: stepping {} failed: {err}", task.id);
            }
        }
        Ok::<(), SandboxError>(())
    })
    .await;
}

/// Teardown: request cancellation for every in-flight task and run one
/// observing step so the `cancelled` transition is audited and persisted
/// (best-effort — an unreachable executor leaves the task to the
/// watchdog).
async fn terminate_inflight(manager: &Arc<SandboxTaskManager>, db_path: &Arc<str>) {
    let m = Arc::clone(manager);
    let _ = call_with_connection(db_path, move |conn| async move {
        let tasks = m.list_recent(&conn, 256)?;
        for task in &tasks {
            if task.status.is_terminal() {
                continue;
            }
            if let Err(err) = m.cancel(&conn, &task.id).await {
                eprintln!("sandbox teardown: cancelling {} failed: {err}", task.id);
            }
            if let Err(err) = m.step(&conn, &task.id).await {
                eprintln!("sandbox teardown: final step of {} failed: {err}", task.id);
            }
        }
        Ok::<(), SandboxError>(())
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    use crate::harness::EventBus;
    use crate::sandbox::fake::FakeSandboxExecutor;
    use crate::sandbox::types::{SandboxLimits, SandboxTaskType};
    use crate::store::repos::workspaces::WorkspaceEntry;
    use crate::store::repos::{work_directories, workspaces};
    use tempfile::TempDir;

    fn file_db() -> (TempDir, Arc<str>) {
        let dir = tempfile::tempdir().unwrap();
        let path: Arc<str> = Arc::from(dir.path().join("sandbox.db").to_string_lossy().to_string());
        let db = Db::open(&path).unwrap();
        crate::store::migrations::run(&db.0).unwrap();
        seed_active_workspace(&db.0);
        (dir, path)
    }

    /// Active workspace with one mounted directory.
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
    }

    fn spec() -> SandboxTaskSpec {
        SandboxTaskSpec {
            task_type: SandboxTaskType::RunCode,
            runtime: "python".to_string(),
            source: "print('hi')".to_string(),
            authorized_dirs: vec!["C:\\proj\\alpha".to_string()],
            limits: SandboxLimits::default(),
        }
    }

    fn plugin_over(
        fake: FakeSandboxExecutor,
        db_path: Arc<str>,
    ) -> (SandboxPlugin, Arc<SandboxTaskManager>) {
        let manager = Arc::new(SandboxTaskManager::new(Arc::new(fake)));
        let plugin = SandboxPlugin::from_manager(Arc::clone(&manager), db_path)
            .with_poll_interval(Duration::from_millis(20));
        (plugin, manager)
    }

    /// 等任务到达终态且产物定位落定（`artifact_ref` 写入或标记
    /// retryable —— 终态迁移与产物回传是同一 step 内的两个事务，
    /// 独立读连接可能短暂观察到中间窗口）。
    async fn wait_settled(service: &SandboxService, task_id: &str) -> SandboxTaskRecord {
        for _ in 0..250 {
            if let Ok(record) = service.get_task(task_id).await {
                if record.status.is_terminal()
                    && (record.artifact_ref.is_some() || record.artifact_retryable)
                {
                    return record;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("task {task_id} did not settle in time");
    }

    #[tokio::test]
    async fn init_registers_service_and_declares_runtimes() {
        let (_dir, db_path) = file_db();
        let (plugin, _manager) = plugin_over(FakeSandboxExecutor::new(), db_path);
        let ctx = Context::new(EventBus::default());

        plugin.init(&ctx).await.unwrap();

        let service = ctx
            .service::<SandboxService>("sandbox")
            .await
            .expect("service registered");
        assert!(service.is_enabled());
        let runtimes = service.runtimes();
        let ids: Vec<&str> = runtimes.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"python") && ids.contains(&"node"));
    }

    #[tokio::test]
    async fn executor_replacement_changes_declared_runtimes() {
        let (_dir, db_path) = file_db();
        let custom = FakeSandboxExecutor::new().with_runtimes(vec![
            crate::sandbox::types::SandboxRuntimeInfo {
                id: "go".to_string(),
                name: "Go".to_string(),
                isolation: "custom".to_string(),
            },
        ]);
        let (plugin, _manager) = plugin_over(custom, db_path);
        let ctx = Context::new(EventBus::default());
        plugin.init(&ctx).await.unwrap();

        let service = ctx.service::<SandboxService>("sandbox").await.unwrap();
        let runtimes = service.runtimes();
        assert_eq!(runtimes.len(), 1);
        assert_eq!(runtimes[0].id, "go");
        // 未声明 runtime 的提交被拒
        let err = service.submit(spec()).await.unwrap_err();
        assert_eq!(err.code(), "sandbox.runtime_unknown");
    }

    #[tokio::test]
    async fn start_drives_submitted_task_to_terminal() {
        let (_dir, db_path) = file_db();
        let (plugin, _manager) = plugin_over(FakeSandboxExecutor::new(), db_path);
        let ctx = Context::new(EventBus::default());
        plugin.init(&ctx).await.unwrap();
        plugin.start().await.unwrap();

        let service = ctx.service::<SandboxService>("sandbox").await.unwrap();
        let record = service.submit(spec()).await.unwrap();
        let done = wait_settled(&service, &record.id).await;

        assert_eq!(
            done.status,
            crate::sandbox::types::SandboxTaskStatus::Succeeded
        );
        assert_eq!(done.exit_info.as_deref(), Some("exit 0"));
        assert!(!done.stdout.is_empty(), "驱动循环应已拉取输出");
        assert!(done.artifact_ref.is_some(), "成功任务应有产物定位");

        plugin.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn disable_hides_entry_but_keeps_reads_and_cancels() {
        let (_dir, db_path) = file_db();
        let (plugin, manager) = plugin_over(FakeSandboxExecutor::new(), db_path);
        let ctx = Context::new(EventBus::default());
        plugin.init(&ctx).await.unwrap();
        plugin.start().await.unwrap();

        let service = ctx.service::<SandboxService>("sandbox").await.unwrap();
        let first = service.submit(spec()).await.unwrap();
        wait_settled(&service, &first.id).await;

        service.set_enabled(false);
        assert!(!service.is_enabled());
        let err = service.submit(spec()).await.unwrap_err();
        assert_eq!(err.code(), "sandbox.unavailable");
        // 读与取消照旧
        assert!(service.get_task(&first.id).await.is_ok());
        assert!(!service.list_tasks(10).await.unwrap().is_empty());
        assert!(service.cancel_task(&first.id).await.is_ok());

        service.set_enabled(true);
        assert!(service.submit(spec()).await.is_ok());
        assert!(manager.is_enabled());

        plugin.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn dispose_terminates_inflight_tasks() {
        let (_dir, db_path) = file_db();
        // 卡在 running 的脚本：最后一项重复，任务永不自行到达终态
        let stuck = FakeSandboxExecutor::new().with_default_script(vec![
            crate::sandbox::executor::SandboxTaskSnapshot::running(),
        ]);
        let (plugin, manager) = plugin_over(stuck, db_path);
        let ctx = Context::new(EventBus::default());
        plugin.init(&ctx).await.unwrap();
        assert!(manager.has_bus(), "init 应挂载内核总线");
        plugin.start().await.unwrap();

        let service = ctx.service::<SandboxService>("sandbox").await.unwrap();
        let record = service.submit(spec()).await.unwrap();
        // 等驱动循环把它推进 running
        for _ in 0..250 {
            let current = service.get_task(&record.id).await.unwrap();
            if current.status == crate::sandbox::types::SandboxTaskStatus::Running {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        plugin.dispose().await.unwrap();

        let done = service.get_task(&record.id).await.unwrap();
        assert_eq!(
            done.status,
            crate::sandbox::types::SandboxTaskStatus::Cancelled,
            "dispose 应终止在途任务并落库"
        );
        assert!(!service.is_enabled(), "dispose 后插件应处于禁用态");
    }

    #[tokio::test]
    async fn dispose_without_start_and_repeated_dispose_are_noops() {
        let (_dir, db_path) = file_db();
        let (plugin, _manager) = plugin_over(FakeSandboxExecutor::new(), db_path);
        // 未 start 直接 dispose
        plugin.dispose().await.unwrap();
        // start → dispose → dispose
        plugin.start().await.unwrap();
        plugin.dispose().await.unwrap();
        plugin.dispose().await.unwrap();
    }

    #[tokio::test]
    async fn repeated_start_spawns_one_driver() {
        let (_dir, db_path) = file_db();
        let (plugin, _manager) = plugin_over(FakeSandboxExecutor::new(), db_path);
        plugin.start().await.unwrap();
        plugin.start().await.unwrap();
        plugin.dispose().await.unwrap();
    }
}
