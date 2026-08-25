//! Notification dispatcher (SPEC bots-telemetry-m1 B3/D6): the second
//! broadcast subscriber on the kernel bus, next to the Tauri event bridge
//! (`lib.rs::forward_event`).
//!
//! Semantics: every enabled non-telemetry integration whose `events`
//! whitelist matches the event topic receives one sink message
//! (title = topic, body = pretty payload truncated to [`BODY_MAX_CHARS`]).
//! An empty whitelist matches every domain topic (`task.*`, `run.*`,
//! `approval.*`, `schedule.*`, `team.*`). Send failures warn and drop —
//! never retried, never blocking, never panicking; broadcast Lagged warns
//! and keeps streaming, mirroring the event bridge.
//!
//! `kind = telemetry` rows are skipped on this path: each ships through its
//! own batched NDJSON [`TelemetryExporter`] started alongside the loop.

use std::sync::Arc;

use nuomi_core::domain::{Integration, IntegrationKind};
use nuomi_core::harness::{Event, EventBus};
use nuomi_core::integrations::telemetry::TelemetryExporter;
use nuomi_core::integrations::{materialize, OutboundSink};
use tokio::sync::broadcast::error::RecvError;
use tokio_util::sync::CancellationToken;

use crate::state::AppState;

/// Upper bound for pretty-printed payloads in notification bodies.
const BODY_MAX_CHARS: usize = 800;

/// Telemetry batch shape defaults (SPEC D4).
const TELEMETRY_BATCH_SIZE: usize = 10;
const TELEMETRY_FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

// ------------------------------------------------------------------ public

/// Spawns the notification dispatcher plus one telemetry exporter per
/// enabled `telemetry` integration.
///
/// The bus subscription happens synchronously before this call returns, so
/// no event published afterwards can slip through; the slow SQLite
/// materialization runs inside the spawned task while early events buffer
/// in the broadcast channel.
pub fn spawn(state: &AppState) {
    let rx = state.kernel.context().subscribe();
    let db_path = state.db_path.clone();
    let bus = state.kernel.context().bus();
    tokio::spawn(async move {
        let (sinks, warnings) = match materialize(db_path).await {
            Ok(pairs) => pairs,
            Err(e) => {
                tracing::warn!(error = %e, "notification dispatcher could not load integrations");
                return;
            }
        };
        for warning in &warnings {
            tracing::warn!(warning = %warning, "integration skipped");
        }

        let mut notify_sinks: Vec<(Integration, Arc<dyn OutboundSink>)> = Vec::new();
        for (integration, sink) in sinks {
            if integration.kind == IntegrationKind::Telemetry {
                start_telemetry_exporter(&integration, bus.clone());
            } else {
                notify_sinks.push((integration, sink));
            }
        }
        if notify_sinks.is_empty() {
            tracing::info!("no notification integrations configured; dispatcher exits");
            return;
        }
        dispatch_loop(rx, notify_sinks).await;
    });
}

// ------------------------------------------------------------------- loop

async fn dispatch_loop(
    mut rx: tokio::sync::broadcast::Receiver<Event>,
    sinks: Vec<(Integration, Arc<dyn OutboundSink>)>,
) {
    loop {
        match rx.recv().await {
            Ok(event) => dispatch_one(&sinks, &event).await,
            Err(RecvError::Lagged(n)) => {
                tracing::warn!(skipped = n, "notification dispatcher lagged behind the bus");
            }
            Err(RecvError::Closed) => break,
        }
    }
}

/// Routes one event to every whitelisted sink; failures warn and drop.
async fn dispatch_one(sinks: &[(Integration, Arc<dyn OutboundSink>)], event: &Event) {
    if !sinks
        .iter()
        .any(|(integration, _)| whitelist_matches(&integration.events, &event.topic))
    {
        return;
    }
    let title = event.topic.as_str();
    let body = body_of(event);
    for (integration, sink) in sinks {
        if !whitelist_matches(&integration.events, &event.topic) {
            continue;
        }
        if let Err(e) = sink.send(title, &body).await {
            // Single failure = drop: no retry, no backpressure, no panic.
            tracing::warn!(
                error = %e,
                sink = %integration.name,
                topic = %event.topic,
                "notification send failed; dropped"
            );
        }
    }
}

/// Starts one NDJSON exporter for an enabled `telemetry` row.
///
/// Token trade-off (deliberate): the shell has no graceful-shutdown hook
/// yet, so the cancellation token is handed straight to the exporter and
/// never cancelled — the detached task lives for the whole process lifetime
/// and dies with it. Keeping the token in AppState only pays off once a
/// shutdown-flush path exists; the exporter already flushes its buffer when
/// the bus closes.
fn start_telemetry_exporter(integration: &Integration, bus: EventBus) {
    let Some(url) = integration
        .config
        .get("webhook_url")
        .and_then(|v| v.as_str())
    else {
        tracing::warn!(
            name = %integration.name,
            "telemetry integration has no webhook_url; exporter not started"
        );
        return;
    };
    let cancel = CancellationToken::new();
    let _exporter = TelemetryExporter::new(url, TELEMETRY_BATCH_SIZE, TELEMETRY_FLUSH_INTERVAL)
        .start(bus, cancel);
}

// ----------------------------------------------------------------- helpers

/// Domain topics eligible for default dispatch (ADR-0002 global namespace).
fn is_domain_topic(topic: &str) -> bool {
    ["task.", "run.", "approval.", "schedule.", "team."]
        .iter()
        .any(|prefix| topic.starts_with(prefix))
}

/// Whitelist match: exact topics only; an empty list matches every domain
/// topic (same empty-means-all semantics as the telemetry namespace filter).
fn whitelist_matches(events: &[String], topic: &str) -> bool {
    if events.is_empty() {
        return is_domain_topic(topic);
    }
    events.iter().any(|t| t == topic)
}

fn truncate_chars(raw: &str, max_chars: usize) -> String {
    if raw.chars().count() <= max_chars {
        raw.to_string()
    } else {
        raw.chars().take(max_chars).collect()
    }
}

/// Message body: pretty-printed payload, char-safe truncated.
fn body_of(event: &Event) -> String {
    let pretty = serde_json::to_string_pretty(&event.payload).unwrap_or_default();
    truncate_chars(&pretty, BODY_MAX_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nuomi_core::integrations::OutboundError;
    use serde_json::json;
    use std::sync::Mutex;

    /// Deterministic recording sink; optionally fails every send.
    struct FakeSink {
        fail: bool,
        calls: Mutex<Vec<(String, String)>>,
    }

    impl FakeSink {
        fn calls(&self) -> Vec<(String, String)> {
            self.calls.lock().expect("fake sink poisoned").clone()
        }
    }

    // Desugared #[async_trait] signature (the macro itself is not a
    // src-tauri dependency).
    impl OutboundSink for FakeSink {
        fn kind(&self) -> IntegrationKind {
            IntegrationKind::QqWebhook
        }

        fn send<'life0, 'life1, 'life2, 'async_trait>(
            &'life0 self,
            title: &'life1 str,
            body: &'life2 str,
        ) -> ::core::pin::Pin<
            Box<
                dyn ::core::future::Future<Output = Result<(), OutboundError>>
                    + ::core::marker::Send
                    + 'async_trait,
            >,
        >
        where
            'life0: 'async_trait,
            'life1: 'async_trait,
            'life2: 'async_trait,
            Self: Sync + 'async_trait,
        {
            Box::pin(async move {
                self.calls
                    .lock()
                    .expect("fake sink poisoned")
                    .push((title.to_string(), body.to_string()));
                if self.fail {
                    Err(OutboundError::Http("boom".into()))
                } else {
                    Ok(())
                }
            })
        }
    }

    /// Builds a whitelisted sink entry plus its concrete assertion handle.
    fn sink_entry(
        name: &str,
        events: &[&str],
        fail: bool,
    ) -> (Integration, Arc<dyn OutboundSink>, std::sync::Arc<FakeSink>) {
        let concrete = Arc::new(FakeSink {
            fail,
            calls: Mutex::new(Vec::new()),
        });
        let trait_obj: Arc<dyn OutboundSink> = concrete.clone();
        let integration = Integration {
            id: format!("i-{name}"),
            name: name.into(),
            kind: IntegrationKind::QqWebhook,
            config: json!({}),
            events: events.iter().map(|s| s.to_string()).collect(),
            enabled: true,
            created_at: 1,
            updated_at: 1,
        };
        (integration, trait_obj, concrete)
    }

    #[test]
    fn whitelist_matches_exactly_and_empty_means_all_domain_topics() {
        assert!(whitelist_matches(
            &["run.state_changed".into()],
            "run.state_changed"
        ));
        assert!(!whitelist_matches(
            &["task.status_changed".into()],
            "run.state_changed"
        ));
        // Empty whitelist: every domain topic matches…
        for topic in [
            "run.state_changed",
            "task.status_changed",
            "approval.requested",
            "schedule.triggered",
            "team.formed",
        ] {
            assert!(whitelist_matches(&[], topic), "{topic} must match");
        }
        // …but session-scoped traffic never does.
        assert!(!whitelist_matches(&[], "session.delta"));
        assert!(!whitelist_matches(&[], "tool.call"));
    }

    #[test]
    fn body_is_pretty_payload_truncated_to_limit() {
        let ev = Event::new("run.state_changed", json!({ "runId": "r1", "to": "ok" }));
        let body = body_of(&ev);
        assert!(body.contains("\"runId\""));
        assert!(body.contains('\n'), "payload must be pretty-printed");

        let big = Event::new(
            "run.state_changed",
            json!({ "blob": "x".repeat(BODY_MAX_CHARS * 3) }),
        );
        assert_eq!(body_of(&big).chars().count(), BODY_MAX_CHARS);
    }

    /// AC5 shape: routing follows each row's whitelist, one sink failing
    /// only logs — the loop continues and later events still dispatch.
    #[tokio::test]
    async fn routes_only_whitelisted_and_survives_sink_failures() {
        let (failing_integration, failing_sink, failing_handle) =
            sink_entry("failing", &["run.state_changed"], true);
        let (healthy_integration, healthy_sink, healthy_handle) = sink_entry("healthy", &[], false);
        let sinks: Vec<(Integration, Arc<dyn OutboundSink>)> = vec![
            (failing_integration, failing_sink),
            (healthy_integration, healthy_sink),
        ];

        dispatch_one(&sinks, &Event::new("run.state_changed", json!({}))).await;
        // The failure above must not stop later dispatches…
        dispatch_one(&sinks, &Event::new("approval.requested", json!({}))).await;

        // …and whitelists decide per row: the failing sink only saw the run
        // topic, the catch-all healthy sink saw both events.
        let failed: Vec<String> = failing_handle
            .calls()
            .into_iter()
            .map(|(title, _)| title)
            .collect();
        assert_eq!(failed, vec!["run.state_changed"]);
        let healthy_titles: Vec<String> = healthy_handle
            .calls()
            .into_iter()
            .map(|(title, _)| title)
            .collect();
        assert_eq!(
            healthy_titles,
            vec!["run.state_changed", "approval.requested"]
        );

        // Non-domain noise reaches nobody with these whitelists.
        dispatch_one(&sinks, &Event::new("session.delta", json!({}))).await;
        assert_eq!(failing_handle.calls().len(), 1);
        assert_eq!(healthy_handle.calls().len(), 2);
    }
}
