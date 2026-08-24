//! Plugin context: typed service registry + event bus access.
//!
//! Services are registered as `Arc<T>` keyed by type name (plus an optional
//! qualifier for multiple services of the same type). Any plugin can resolve
//! a previously registered service — the Cordis "ctx.service" pattern.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use super::{Event, EventBus, HarnessError};

type ServiceMap = HashMap<(TypeId, String), (String, Arc<dyn Any + Send + Sync>)>;

/// Shared context handed to every plugin during `init`.
#[derive(Clone)]
pub struct Context {
    services: Arc<RwLock<ServiceMap>>,
    bus: EventBus,
}

impl Default for Context {
    fn default() -> Self {
        Self::new(EventBus::default())
    }
}

impl Context {
    pub fn new(bus: EventBus) -> Self {
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            bus,
        }
    }

    /// Registers a service under type `T` and an optional qualifier.
    /// Fails if the same `(type, qualifier)` slot is already taken.
    pub async fn register_service<T: Any + Send + Sync>(
        &self,
        owner: &str,
        qualifier: &str,
        service: Arc<T>,
    ) -> Result<(), HarnessError> {
        let key = (TypeId::of::<T>(), qualifier.to_string());
        let mut map = self.services.write().await;
        if let Some((existing_owner, _)) = map.get(&key) {
            return Err(HarnessError::DuplicateService {
                name: format!("{}::{qualifier}", std::any::type_name::<T>()),
                owner: existing_owner.clone(),
            });
        }
        map.insert(key, (owner.to_string(), service));
        Ok(())
    }

    /// Resolves the service registered for `T` with `qualifier`.
    pub async fn service<T: Any + Send + Sync>(&self, qualifier: &str) -> Option<Arc<T>> {
        let key = (TypeId::of::<T>(), qualifier.to_string());
        let map = self.services.read().await;
        let (_, boxed) = map.get(&key)?;
        boxed.clone().downcast::<T>().ok()
    }

    /// Publishes an event on the kernel bus.
    pub fn publish(&self, event: Event) {
        self.bus.publish(event);
    }

    /// Subscribes to the kernel bus.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Event> {
        self.bus.subscribe()
    }

    /// Number of registered services (introspection/testing).
    pub async fn service_count(&self) -> usize {
        self.services.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Greeter {
        prefix: String,
    }

    #[tokio::test]
    async fn registers_and_resolves_typed_service() {
        let ctx = Context::default();
        ctx.register_service(
            "greeter-plugin",
            "",
            Arc::new(Greeter {
                prefix: "hi".into(),
            }),
        )
        .await
        .unwrap();
        let svc = ctx.service::<Greeter>("").await.expect("service present");
        assert_eq!(svc.prefix, "hi");
        assert_eq!(ctx.service_count().await, 1);
    }

    #[tokio::test]
    async fn duplicate_registration_is_rejected_but_qualifiers_coexist() {
        let ctx = Context::default();
        ctx.register_service("a", "one", Arc::new(Greeter { prefix: "1".into() }))
            .await
            .unwrap();
        assert!(ctx
            .register_service("b", "one", Arc::new(Greeter { prefix: "2".into() }))
            .await
            .is_err());
        ctx.register_service("a", "two", Arc::new(Greeter { prefix: "2".into() }))
            .await
            .unwrap();
        assert_eq!(ctx.service_count().await, 2);
    }

    #[tokio::test]
    async fn events_reach_subscribers() {
        let ctx = Context::default();
        let mut rx = ctx.subscribe();
        ctx.publish(Event::new("test.tick", serde_json::json!({ "n": 1 })));
        let ev = rx.recv().await.unwrap();
        assert_eq!(ev.topic, "test.tick");
        assert_eq!(ev.payload["n"], 1);
    }
}
