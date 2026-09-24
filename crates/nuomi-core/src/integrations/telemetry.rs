//! Telemetry exporter: collects kernel bus events and ships them as NDJSON
//! batches (`{"topic":...,"payload":...,"ts":...}` per line) to a remote
//! endpoint.
//!
//! The topic filter mirrors the global "domain" namespace semantics of the
//! desktop-shell event bridge (`src-tauri/src/events.rs::is_domain_topic`)
//! but is implemented independently here so nuomi-core stays UI-free.

use std::time::Duration;

use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use crate::domain::now_ms;
use crate::harness::{Event, EventBus};

pub struct TelemetryExporter {
    endpoint: String,
    batch_size: usize,
    flush_interval: Duration,
    client: reqwest::Client,
}

impl TelemetryExporter {
    pub fn new(endpoint: impl Into<String>, batch_size: usize, flush_interval: Duration) -> Self {
        Self {
            endpoint: endpoint.into(),
            batch_size: batch_size.max(1),
            flush_interval,
            client: super::http_client(),
        }
    }

    /// Spawns the collect/flush loop. Subscribes synchronously, so every
    /// event published after `start()` returns is captured. On cancel (or bus
    /// closure) any buffered events are flushed once, then the task exits
    /// returning the number of events successfully delivered over its
    /// lifetime.
    pub fn start(self, bus: EventBus, cancel: CancellationToken) -> tokio::task::JoinHandle<u64> {
        let mut rx = bus.subscribe();
        tokio::spawn(async move {
            let mut buffer: Vec<String> = Vec::new();
            let mut delivered = 0u64;
            let mut ticker = tokio::time::interval(self.flush_interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                let interval_due = tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = ticker.tick() => true,
                    recv = rx.recv() => match recv {
                        Ok(event) => {
                            if is_exportable_topic(&event.topic) {
                                buffer.push(ndjson_line(&event));
                                if buffer.len() >= self.batch_size {
                                    delivered += flush(&mut buffer, &self.client, &self.endpoint).await;
                                }
                            }
                            continue;
                        }
                        Err(RecvError::Lagged(n)) => {
                            tracing::warn!(dropped = n, "telemetry exporter lagged behind the bus");
                            continue;
                        }
                        Err(RecvError::Closed) => break,
                    },
                };
                if interval_due && !buffer.is_empty() {
                    delivered += flush(&mut buffer, &self.client, &self.endpoint).await;
                }
            }
            // Final drain on cancel/bus-close: ship whatever remains.
            if !buffer.is_empty() {
                delivered += flush(&mut buffer, &self.client, &self.endpoint).await;
            }
            delivered
        })
    }
}

/// Topics eligible for export — the low-frequency structured namespace
/// (`task.*`, `run.*`, `approval.*`, `schedule.*`, `team.*`). Session-scoped
/// high-frequency traffic (`session.*`, `tool.*`, `hook.*`) is never shipped.
fn is_exportable_topic(topic: &str) -> bool {
    topic.starts_with("task.")
        || topic.starts_with("run.")
        || topic.starts_with("approval.")
        || topic.starts_with("schedule.")
        || topic.starts_with("team.")
        || topic.starts_with("perf.")
}

fn ndjson_line(event: &Event) -> String {
    serde_json::json!({
        "topic": event.topic,
        "payload": event.payload,
        "ts": now_ms(),
    })
    .to_string()
}

/// Masks an endpoint URL for logging (SPEC M-BOT1 D5: no full URL above
/// DEBUG). Keeps `scheme://host/`; every path segment collapses to its first
/// character except the last, which also keeps its trailing 4 characters;
/// query/fragment are dropped outright. Unparseable input degrades to
/// `"<masked>"` so nothing sensitive can leak through warn-level logs.
fn masked(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "<masked>".to_string();
    };
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let (host, path) = match rest.split_once('/') {
        Some((host, path)) => (host, path),
        None => (rest, ""),
    };
    if scheme.is_empty() || host.is_empty() {
        return "<masked>".to_string();
    }
    let mut segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let mut out = format!("{scheme}://{host}");
    let Some(last) = segments.pop() else {
        out.push('/');
        return out;
    };
    for seg in segments {
        out.push('/');
        if let Some(c) = seg.chars().next() {
            out.push(c);
        }
    }
    out.push('/');
    if let Some(c) = last.chars().next() {
        out.push(c);
    }
    let tail_start = last.chars().count().saturating_sub(4);
    let tail: String = last.chars().skip(tail_start).collect();
    out.push_str(&tail);
    out
}

/// POSTs the buffered lines as one NDJSON body. The batch is always dropped
/// afterwards: HTTP failures warn and discard (never accumulate backlog).
async fn flush(buffer: &mut Vec<String>, client: &reqwest::Client, endpoint: &str) -> u64 {
    let body = buffer.join("\n");
    let count = buffer.len() as u64;
    buffer.clear();
    match client
        .post(endpoint)
        .header(reqwest::header::CONTENT_TYPE, "application/x-ndjson")
        .body(body)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => count,
        Ok(resp) => {
            tracing::warn!(
                status = %resp.status(),
                endpoint = %masked(endpoint),
                "telemetry export failed; dropping batch"
            );
            0
        }
        Err(e) if e.is_timeout() => {
            tracing::warn!(endpoint = %masked(endpoint), "telemetry export timed out; dropping batch");
            0
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                endpoint = %masked(endpoint),
                "telemetry export failed; dropping batch"
            );
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::sync::{Arc, Mutex};
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    type Captured = Arc<Mutex<Vec<String>>>;

    async fn ndjson_server() -> (MockServer, Captured) {
        let server = MockServer::start().await;
        let captured: Captured = Arc::new(Mutex::new(Vec::new()));
        let sink = captured.clone();
        Mock::given(method("POST"))
            .respond_with(move |req: &wiremock::Request| {
                sink.lock()
                    .unwrap()
                    .push(String::from_utf8(req.body.to_vec()).unwrap_or_default());
                ResponseTemplate::new(200)
            })
            .mount(&server)
            .await;
        (server, captured)
    }

    async fn wait_for(captured: &Captured, min: usize) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            if captured.lock().unwrap().len() >= min {
                return;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for telemetry POSTs"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn parse_lines(raw: &str) -> Vec<Value> {
        raw.lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn table_topic_filter_matches_only_domain_namespace() {
        for topic in [
            "task.status_changed",
            "run.state_changed",
            "approval.requested",
            "schedule.triggered",
            "team.formed",
        ] {
            assert!(is_exportable_topic(topic), "{topic} must be exported");
        }
        for topic in [
            "session.delta",
            "tool.call",
            "hook.pre_tool_call",
            "tasks.x",
            "team",
            "",
        ] {
            assert!(!is_exportable_topic(topic), "{topic} must be filtered");
        }
    }

    #[test]
    fn masked_keeps_scheme_host_and_trims_path_segments() {
        let cases: &[(&str, &str)] = &[
            (
                "https://open.feishu.cn/open-apis/bot/v2/hook/abc123def456",
                "https://open.feishu.cn/o/b/v/h/af456",
            ),
            (
                "http://localhost:8080/api/ingest",
                "http://localhost:8080/a/igest",
            ),
            ("https://gw.internal/top", "https://gw.internal/ttop"),
            ("https://bare.host", "https://bare.host/"),
            (
                "https://q.host/path?token=secret#frag",
                "https://q.host/ppath",
            ),
            ("not-a-url", "<masked>"),
            ("://no-scheme", "<masked>"),
        ];
        for (raw, want) in cases {
            assert_eq!(masked(raw), *want, "masking {raw}");
        }
    }

    #[tokio::test]
    async fn full_batch_flushes_single_ndjson_post_and_filters_topics() {
        let (server, captured) = ndjson_server().await;
        let bus = EventBus::default();
        let cancel = CancellationToken::new();
        // long interval so only the full-batch path can flush during the test
        let handle = TelemetryExporter::new(server.uri(), 2, Duration::from_secs(60))
            .start(bus.clone(), cancel.clone());

        bus.publish(Event::new(
            "task.status_changed",
            serde_json::json!({ "n": 1 }),
        ));
        bus.publish(Event::new(
            "session.delta",
            serde_json::json!({ "noise": true }),
        ));
        bus.publish(Event::new(
            "run.state_changed",
            serde_json::json!({ "n": 2 }),
        ));

        wait_for(&captured, 1).await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        let posts: Vec<String> = captured.lock().unwrap().clone();
        assert_eq!(posts.len(), 1, "exactly one batch POST");
        let lines = parse_lines(&posts[0]);
        assert_eq!(lines.len(), 2, "two exported events, noise excluded");
        assert_eq!(lines[0]["topic"], "task.status_changed");
        assert_eq!(lines[1]["topic"], "run.state_changed");
        assert!(lines.iter().all(|l| l["ts"].is_i64()));

        cancel.cancel();
        handle.await.unwrap();
        assert_eq!(captured.lock().unwrap().len(), 1, "no further POSTs");
    }

    #[tokio::test]
    async fn cancel_flushes_remaining_buffer_then_exits() {
        let (server, captured) = ndjson_server().await;
        let bus = EventBus::default();
        let cancel = CancellationToken::new();
        // huge batch + long interval: only the cancel-drain can flush
        let handle = TelemetryExporter::new(server.uri(), 1000, Duration::from_secs(60))
            .start(bus.clone(), cancel.clone());

        bus.publish(Event::new(
            "approval.requested",
            serde_json::json!({ "id": "a1" }),
        ));
        // let the exporter collect before cancelling
        tokio::time::sleep(Duration::from_millis(200)).await;
        cancel.cancel();

        let delivered = handle.await.unwrap();
        wait_for(&captured, 1).await;
        assert_eq!(delivered, 1);
        let lines = parse_lines(&captured.lock().unwrap()[0]);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["topic"], "approval.requested");
    }

    #[tokio::test]
    async fn failing_endpoint_drops_batch_without_backlog() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let bus = EventBus::default();
        let cancel = CancellationToken::new();
        let handle = TelemetryExporter::new(server.uri(), 1, Duration::from_secs(60))
            .start(bus.clone(), cancel.clone());

        bus.publish(Event::new("task.status_changed", serde_json::json!({})));
        bus.publish(Event::new("task.status_changed", serde_json::json!({})));
        tokio::time::sleep(Duration::from_millis(200)).await;

        // both batches were attempted against the failing endpoint…
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if !server
                .received_requests()
                .await
                .unwrap_or_default()
                .is_empty()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(!server
            .received_requests()
            .await
            .unwrap_or_default()
            .is_empty());
        // …but nothing was delivered, so nothing accumulated.
        cancel.cancel();
        assert_eq!(handle.await.unwrap(), 0);
    }
}
