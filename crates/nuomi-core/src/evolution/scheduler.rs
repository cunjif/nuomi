//! Periodic background research loop (SPEC T11 "定时任务"):
//! ticks on an interval, checks the persisted authorization switch each
//! round, and only then issues allowlisted fetches. Cancellation-safe.

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::plugins::MemoryService;

use super::research::{online_authorized, ResearchReportEntry, ResearchScheduler};

/// A cancellable periodic research task.
pub struct PeriodicResearch {
    scheduler: Arc<ResearchScheduler>,
    memory: MemoryService,
    interval: Duration,
    topic: String,
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
            cancel: CancellationToken::new(),
        }
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
