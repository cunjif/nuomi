//! Kernel event bus: in-process broadcast of domain events.
//!
//! Persistence of events (EventRecord) is the concern of the store layer;
//! the bus only carries live notifications. Producers must persist first,
//! then publish ("persist before side effects" iron rule).

use serde_json::Value;

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

/// Broadcast hub. Bounded channel: slow subscribers drop oldest events
/// (live streams only; durable history always lives in SQLite).
#[derive(Clone)]
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<Event>,
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(capacity);
        Self { tx }
    }

    /// Publishes an event to all current subscribers.
    pub fn publish(&self, event: Event) {
        // A send error only means there are no receivers; that is fine.
        let _ = self.tx.send(event);
    }

    /// Subscribes to all events.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}
