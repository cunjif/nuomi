//! Kernel event bus: in-process broadcast of domain events, plus an optional
//! waterfall (onion-middleware) chain over the same topics.
//!
//! Persistence of events (EventRecord) is the concern of the store layer;
//! the bus only carries live notifications. Producers must persist first,
//! then publish ("persist before side effects" iron rule).
//!
//! Two dispatch modes coexist:
//! - [`EventBus::publish`]/[`EventBus::subscribe`]: fire-and-forget broadcast,
//!   semantics unchanged.
//! - [`EventBus::waterfall`]: the event walks a chain of async handlers in
//!   registration order (only those whose topic pattern matches). Each handler
//!   may mutate the payload and pass it on, or reject it — a rejected event
//!   stops propagating and the publisher receives an error.

use std::sync::Arc;

use futures::future::BoxFuture;
use serde_json::Value;
use tokio::sync::RwLock;

/// A domain event flowing through the bus.
#[derive(Debug, Clone)]
pub struct Event {
    /// Dot-separated topic, e.g. `session.message`, `tool.call`, `run.state`.
    pub topic: String,
    /// JSON payload (schema owned by the emitting component).
    pub payload: Value,
}

impl Event {
    pub fn new(topic: impl Into<String>, payload: Value) -> Self {
        Self {
            topic: topic.into(),
            payload,
        }
    }
}

/// Control returned by a waterfall handler: pass the (possibly mutated) event
/// to the next handler, or veto it.
#[derive(Debug)]
pub enum Flow {
    Next(Event),
    Reject,
}

/// Async waterfall handler. Receives the current event by value.
pub type WaterfallHandler = Arc<dyn Fn(Event) -> BoxFuture<'static, Flow> + Send + Sync>;

/// Returned by [`EventBus::waterfall`] when a handler vetoes the event.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("event on topic '{topic}' rejected by waterfall handler '{handler}'")]
pub struct EventRejected {
    pub topic: String,
    pub handler: String,
}

/// One registered waterfall stage.
struct Stage {
    /// Topic prefix this stage applies to (`""` or `"*"` matches everything;
    /// `"run"` matches `run.state` but not `running`).
    pattern: String,
    name: String,
    handler: WaterfallHandler,
}

/// Broadcast hub. Bounded channel: slow subscribers drop oldest events
/// (live streams only; durable history always lives in SQLite).
#[derive(Clone)]
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<Event>,
    stages: Arc<RwLock<Vec<Stage>>>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(capacity);
        Self {
            tx,
            stages: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Publishes an event to all current subscribers (broadcast semantics —
    /// untouched by the waterfall chain).
    pub fn publish(&self, event: Event) {
        // A send error only means there are no receivers; that is fine.
        let _ = self.tx.send(event);
    }

    /// Subscribes to all events.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    /// Appends a waterfall stage for events whose topic matches `pattern`
    /// (topic-prefix match on segment boundaries). Stages run in registration
    /// order for every matching event.
    pub async fn on(&self, pattern: &str, name: impl Into<String>, handler: WaterfallHandler) {
        self.stages.write().await.push(Stage {
            pattern: pattern.to_string(),
            name: name.into(),
            handler,
        });
    }

    /// Runs `event` through the waterfall chain: every stage whose pattern
    /// matches the topic, in registration order. A stage that returns
    /// [`Flow::Reject`] stops propagation and the event never reaches later
    /// stages; the publisher gets an [`EventRejected`]. On success the final
    /// (possibly payload-mutated) event is returned.
    pub async fn waterfall(&self, event: Event) -> Result<Event, EventRejected> {
        // Snapshot matching stages so handlers run outside the lock.
        let stages: Vec<Stage> = {
            let all = self.stages.read().await;
            all.iter()
                .filter(|s| topic_matches(&s.pattern, &event.topic))
                .map(|s| Stage {
                    pattern: s.pattern.clone(),
                    name: s.name.clone(),
                    handler: Arc::clone(&s.handler),
                })
                .collect()
        };
        let mut current = event;
        for stage in &stages {
            // The handler consumes the event; capture the topic so a veto can
            // still report which event was rejected.
            let topic = current.topic.clone();
            match (stage.handler)(current).await {
                Flow::Next(next) => current = next,
                Flow::Reject => {
                    return Err(EventRejected {
                        topic,
                        handler: stage.name.clone(),
                    });
                }
            }
        }
        Ok(current)
    }
}

/// Prefix match on dot-segment boundaries: `"run"` matches `run` and
/// `run.state`; `""` and `"*"` match everything; `"run.st"` matches nothing
/// beyond exact `"run.st"`.
fn topic_matches(pattern: &str, topic: &str) -> bool {
    if pattern.is_empty() || pattern == "*" {
        return true;
    }
    topic == pattern
        || topic
            .strip_prefix(pattern)
            .is_some_and(|rest| rest.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass_through() -> WaterfallHandler {
        Arc::new(|ev: Event| Box::pin(async move { Flow::Next(ev) }))
    }

    #[tokio::test]
    async fn broadcast_semantics_are_unchanged() {
        let bus = EventBus::default();
        bus.on(
            "",
            "veto-all",
            Arc::new(|_: Event| Box::pin(async { Flow::Reject })),
        )
        .await;
        // publish/subscribe is independent of any waterfall stage.
        let mut rx = bus.subscribe();
        bus.publish(Event::new("t.a", serde_json::json!(1)));
        assert_eq!(rx.recv().await.unwrap().payload, serde_json::json!(1));
    }

    #[tokio::test]
    async fn waterfall_runs_matching_stages_in_order() {
        let bus = EventBus::default();
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));
        for (pattern, name) in [("run", "a"), ("", "b-global"), ("other", "c-skip")] {
            let log = Arc::clone(&log);
            bus.on(
                pattern,
                name,
                Arc::new(move |ev: Event| {
                    let log = Arc::clone(&log);
                    let name = name.to_string();
                    Box::pin(async move {
                        log.lock().unwrap().push(name);
                        Flow::Next(ev)
                    })
                }),
            )
            .await;
        }
        bus.waterfall(Event::new("run.state", serde_json::json!({})))
            .await
            .unwrap();
        assert_eq!(*log.lock().unwrap(), vec!["a", "b-global"]);
    }

    #[tokio::test]
    async fn waterfall_handler_can_mutate_payload() {
        let bus = EventBus::default();
        bus.on(
            "run",
            "adder",
            Arc::new(|ev: Event| {
                Box::pin(async move {
                    let mut ev = ev;
                    ev.payload["n"] = serde_json::json!(ev.payload["n"].as_i64().unwrap_or(0) + 1);
                    Flow::Next(ev)
                })
            }),
        )
        .await;
        let out = bus
            .waterfall(Event::new("run.state", serde_json::json!({ "n": 1 })))
            .await
            .unwrap();
        assert_eq!(out.payload["n"], 2);
    }

    #[tokio::test]
    async fn rejection_stops_propagation_and_reports() {
        let bus = EventBus::default();
        bus.on(
            "run",
            "gate",
            Arc::new(|_: Event| Box::pin(async { Flow::Reject })),
        )
        .await;
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        bus.on(
            "run",
            "after-gate",
            Arc::new(move |ev: Event| {
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
                Box::pin(async move { Flow::Next(ev) })
            }),
        )
        .await;
        let err = bus
            .waterfall(Event::new("run.state", serde_json::json!({})))
            .await
            .unwrap_err();
        assert_eq!(err.handler, "gate");
        assert_eq!(err.topic, "run.state");
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
        // Non-matching topics are unaffected.
        assert!(bus
            .waterfall(Event::new("tool.call", serde_json::json!({})))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn empty_chain_returns_event() {
        let bus = EventBus::default();
        let ev = Event::new("t.x", serde_json::json!({}));
        let out = bus.waterfall(ev.clone()).await.unwrap();
        assert_eq!(out.topic, ev.topic);
    }

    #[tokio::test]
    async fn topic_prefix_matching_respects_segment_boundaries() {
        assert!(topic_matches("run", "run.state"));
        assert!(topic_matches("run", "run"));
        assert!(!topic_matches("run", "running"));
        assert!(!topic_matches("run.st", "run.state"));
        assert!(topic_matches("", "anything"));
        assert!(topic_matches("*", "anything"));
        assert!(pass_through()({
            // handler type sanity: callable with an event
            Event::new("t", serde_json::json!({}))
        })
        .await
        .is_next_like());
    }

    impl Flow {
        fn is_next_like(&self) -> bool {
            matches!(self, Flow::Next(_))
        }
    }
}
