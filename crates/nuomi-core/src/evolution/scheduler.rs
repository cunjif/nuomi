//! Periodic background research loop (SPEC T11 "定时任务"):
//! ticks on an interval, checks the persisted authorization switch each
//! round, and only then issues allowlisted fetches. Cancellation-safe.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::plugins::MemoryService;

use super::research::{online_authorized, ResearchReportEntry, ResearchScheduler};

/// Default per-target cooldown window for automatic evolution triggers
/// (prime-agent stability package: runaway loops must self-throttle).
pub const DEFAULT_COOLDOWN: Duration = Duration::from_secs(20 * 60);

/// Result of a cooldown check for one automatic trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CooldownDecision {
    /// Trigger may proceed; the target's window timer is refreshed.
    Allow,
    /// Trigger is inside the cooldown window and must be skipped.
    Skip { remaining: Duration },
}

/// Per-target cooldown registry for *automatic* triggers.
///
/// Each target (plugin name, topic, …) may fire at most once per window.
/// Manual triggers bypass the check entirely but still refresh the timer,
/// so a manual run does not let the next automatic run fire early.
pub struct CooldownGate {
    window: Duration,
    last_fired: Mutex<HashMap<String, std::time::Instant>>,
}

impl CooldownGate {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            last_fired: Mutex::new(HashMap::new()),
        }
    }

    /// Gate with the default 20-minute window.
    pub fn with_default_window() -> Self {
        Self::new(DEFAULT_COOLDOWN)
    }

    /// Checks (and, when allowed, refreshes) the cooldown for `target`.
    /// `manual = true` bypasses the window — manual runs are never skipped.
    pub fn check(&self, target: &str, manual: bool) -> CooldownDecision {
        let now = std::time::Instant::now();
        let mut last = self
            .last_fired
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !manual {
            if let Some(fired) = last.get(target) {
                let elapsed = now.duration_since(*fired);
                if elapsed < self.window {
                    return CooldownDecision::Skip {
                        remaining: self.window - elapsed,
                    };
                }
            }
        }
        last.insert(target.to_string(), now);
        CooldownDecision::Allow
    }
}

/// A cancellable periodic research task.
pub struct PeriodicResearch {
    scheduler: Arc<ResearchScheduler>,
    memory: MemoryService,
    interval: Duration,
    topic: String,
    /// Optional per-target cooldown; `None` keeps the legacy always-run
    /// behavior. When set, automatic ticks inside the window are skipped
    /// and the skip is logged.
    cooldown: Option<Arc<CooldownGate>>,
    cancel: CancellationToken,
}

impl PeriodicResearch {
    pub fn new(
        scheduler: Arc<ResearchScheduler>,
        memory: MemoryService,
        interval: Duration,
        topic: impl Into<String>,
    ) -> Self {
        Self {
            scheduler,
            memory,
            interval,
            topic: topic.into(),
            cooldown: None,
            cancel: CancellationToken::new(),
        }
    }

    /// Attaches a per-target cooldown gate; ticks inside the window are
    /// skipped (with a logged reason) instead of fetching.
    pub fn with_cooldown(mut self, cooldown: CooldownGate) -> Self {
        self.cooldown = Some(Arc::new(cooldown));
        self
    }

    /// Handle that cancels the loop on drop.
    pub fn cancel_handle(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// Spawns the tick loop; `on_report` receives each successful batch.
    /// Authorization is re-checked every tick, so revoking the switch
    /// stops network activity within one interval without restarting.
    pub fn spawn<F>(&self, on_report: F) -> tokio::task::JoinHandle<()>
    where
        F: Fn(Vec<ResearchReportEntry>) + Send + 'static,
    {
        let scheduler = self.scheduler.clone();
        let memory = self.memory.clone();
        let interval = self.interval;
        let topic = self.topic.clone();
        let cooldown = self.cooldown.clone();
        let cancel = self.cancel.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            ticker.tick().await; // first tick fires immediately — skip it
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = ticker.tick() => {
                        if !online_authorized(&memory).await {
                            continue;
                        }
                        // Automatic trigger: honor the per-target cooldown
                        // window and log the skip reason when throttled.
                        if let Some(gate) = &cooldown {
                            match gate.check(&topic, false) {
                                CooldownDecision::Allow => {}
                                CooldownDecision::Skip { remaining } => {
                                    tracing::info!(
                                        target = %topic,
                                        remaining_secs = remaining.as_secs(),
                                        "periodic research skipped: cooldown window active"
                                    );
                                    continue;
                                }
                            }
                        }
                        match scheduler.run_once(&topic, true).await {
                            Ok(entries) if !entries.is_empty() => on_report(entries),
                            Ok(_) => {}
                            Err(e) => {
                                tracing::warn!(error = %e, "periodic research pass failed");
                            }
                        }
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evolution::research::{set_online_authorized, ResearchFetcher};
    use std::collections::HashMap;
    use wiremock::matchers::*;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn cooldown_gate_blocks_automatic_but_not_manual_triggers() {
        let gate = CooldownGate::new(Duration::from_millis(50));
        assert_eq!(gate.check("system_prompt", false), CooldownDecision::Allow);
        assert!(matches!(
            gate.check("system_prompt", false),
            CooldownDecision::Skip { .. }
        ));
        // Each target has its own window.
        assert_eq!(gate.check("other", false), CooldownDecision::Allow);
        // Manual triggers bypass the window…
        assert_eq!(gate.check("system_prompt", true), CooldownDecision::Allow);
        // …but refresh it, so the next automatic trigger stays throttled.
        assert!(matches!(
            gate.check("system_prompt", false),
            CooldownDecision::Skip { .. }
        ));
        // Window expiry re-allows automatic triggers.
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(gate.check("system_prompt", false), CooldownDecision::Allow);
    }

    #[test]
    fn default_cooldown_window_is_twenty_minutes() {
        assert_eq!(DEFAULT_COOLDOWN, Duration::from_secs(20 * 60));
        assert!(matches!(
            CooldownGate::with_default_window().check("t", false),
            CooldownDecision::Allow
        ));
    }

    #[tokio::test]
    async fn cooldown_throttles_periodic_automatic_ticks() {
        let server = MockServer::start().await;
        // Exactly one fetch may pass: the first authorized tick. Every later
        // tick falls inside the cooldown window and must be skipped.
        server
            .register(
                Mock::given(any())
                    .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
                    .expect(1),
            )
            .await;

        let dir = tempfile::tempdir().unwrap();
        let mem = MemoryService::new(dir.path().join("cd.db").to_string_lossy().to_string());
        set_online_authorized(&mem, true).await.unwrap();

        let overrides = HashMap::from([("github.com".to_string(), server.uri())]);
        let fetcher = Arc::new(ResearchFetcher::with_overrides(
            reqwest::Client::new(),
            overrides,
        ));
        let sched = Arc::new(ResearchScheduler::new(fetcher, ["github.com"]));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<usize>();
        let periodic = PeriodicResearch::new(sched, mem, Duration::from_millis(10), "t")
            .with_cooldown(CooldownGate::new(Duration::from_secs(3600)));
        let handle = periodic.spawn(move |entries| {
            let _ = tx.send(entries.len());
        });

        tokio::time::sleep(Duration::from_millis(80)).await;
        periodic.cancel.cancel();
        let _ = handle.await;

        assert_eq!(
            rx.recv().await,
            Some(1),
            "the first authorized tick must fetch"
        );
        assert!(
            rx.try_recv().is_err(),
            "ticks inside the cooldown window must be skipped"
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn periodic_loop_fetches_when_authorized_and_stops_on_cancel() {
        let server = MockServer::start().await;
        server
            .register(
                Mock::given(any()).respond_with(ResponseTemplate::new(200).set_body_string("ok")),
            )
            .await;

        let dir = tempfile::tempdir().unwrap();
        let mem = MemoryService::new(dir.path().join("a.db").to_string_lossy().to_string());
        set_online_authorized(&mem, true).await.unwrap();

        let overrides = HashMap::from([("github.com".to_string(), server.uri())]);
        let fetcher = Arc::new(ResearchFetcher::with_overrides(
            reqwest::Client::new(),
            overrides,
        ));
        let sched = Arc::new(ResearchScheduler::new(fetcher, ["github.com"]));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<usize>();
        let periodic = PeriodicResearch::new(sched, mem.clone(), Duration::from_millis(20), "gepa");
        let handle = periodic.spawn(move |entries| {
            let _ = tx.send(entries.len());
        });

        let mut batches = 0usize;
        for _ in 0..50 {
            if tokio::time::timeout(Duration::from_millis(40), rx.recv())
                .await
                .is_ok()
            {
                batches += 1;
                if batches >= 2 {
                    break;
                }
            }
        }
        periodic.cancel.cancel();
        let _ = handle.await;
        assert!(
            batches >= 2,
            "expected at least 2 report batches, got {batches}"
        );
    }

    #[tokio::test]
    async fn unauthorized_periodic_loop_never_fetches() {
        let server = MockServer::start().await;
        server
            .register(
                Mock::given(any())
                    .respond_with(ResponseTemplate::new(200))
                    .expect(0),
            )
            .await;

        let dir = tempfile::tempdir().unwrap();
        let mem = MemoryService::new(dir.path().join("b.db").to_string_lossy().to_string());
        // Authorization stays false.

        let overrides = HashMap::from([("github.com".to_string(), server.uri())]);
        let fetcher = Arc::new(ResearchFetcher::with_overrides(
            reqwest::Client::new(),
            overrides,
        ));
        let sched = Arc::new(ResearchScheduler::new(fetcher, ["github.com"]));

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<usize>();
        let periodic = PeriodicResearch::new(sched, mem, Duration::from_millis(10), "t");
        let handle = periodic.spawn(move |entries| {
            let _ = tx.send(entries.len());
        });
        tokio::time::sleep(Duration::from_millis(60)).await;
        periodic.cancel.cancel();
        let _ = handle.await;
        assert!(
            rx.try_recv().is_err(),
            "no reports may arrive while unauthorized"
        );
        server.verify().await;
    }
}
