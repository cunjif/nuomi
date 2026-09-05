//! Plugin context: typed service registry + event bus access + effects.
//!
//! Services are registered as `Arc<T>` keyed by type name (plus an optional
//! qualifier for multiple services of the same type). Any plugin can resolve
//! a previously registered service — the Cordis "ctx.service" pattern.
//!
//! Effects are the Cordis "ctx.effect" pattern: every resource a plugin
//! registers (service, listener, task) carries an async disposer. Disposers
//! are stacked per owner and run in LIFO order on teardown, so cleanup is the
//! exact reverse of registration.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use futures::future::BoxFuture;
use tokio::sync::RwLock;

use super::{Event, EventBus, HarnessError};

type ServiceMap = HashMap<(TypeId, String), (String, Arc<dyn Any + Send + Sync>)>;

/// An async teardown function registered via [`Context::effect`].
/// `Sync` keeps every `Context` handle shareable across await points.
pub type Disposer = Box<dyn FnOnce() -> BoxFuture<'static, Result<(), HarnessError>> + Send + Sync>;

/// Handle returned by [`Context::effect`], identifying the registered effect.
/// Pass it back to [`Context::dispose_effect`] to tear down a single effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectGuard {
    owner: String,
    tag: String,
}

impl EffectGuard {
    pub fn owner(&self) -> &str {
        &self.owner
    }
    pub fn tag(&self) -> &str {
        &self.tag
    }
}

/// One registered side effect: a correlation tag plus its async disposer.
struct Effect {
    tag: String,
    disposer: Disposer,
}

/// Shared context handed to every plugin during `init`.
#[derive(Clone)]
pub struct Context {
    services: Arc<RwLock<ServiceMap>>,
    /// Per-owner disposer stacks; teardown pops in LIFO order.
    effects: Arc<RwLock<HashMap<String, Vec<Effect>>>>,
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
            effects: Arc::new(RwLock::new(HashMap::new())),
            bus,
        }
    }

    /// Registers a service under type `T` and an optional qualifier.
    /// Fails if the same `(type, qualifier)` slot is already taken.
    ///
    /// Internally registers an effect whose disposer removes the service, so
    /// `dispose_owner` (or `unregister_service`) tears it down reversibly.
    pub async fn register_service<T: Any + Send + Sync>(
        &self,
        owner: &str,
        qualifier: &str,
        service: Arc<T>,
    ) -> Result<(), HarnessError> {
        let key = (TypeId::of::<T>(), qualifier.to_string());
        {
            let mut map = self.services.write().await;
            if let Some((existing_owner, _)) = map.get(&key) {
                return Err(HarnessError::DuplicateService {
                    name: format!("{}::{qualifier}", std::any::type_name::<T>()),
                    owner: existing_owner.clone(),
                });
            }
            map.insert(key.clone(), (owner.to_string(), service));
        }
        let tag = format!("service:{}::{qualifier}", std::any::type_name::<T>());
        let drop_key = key;
        let services = Arc::clone(&self.services);
        self.effect(
            owner,
            tag,
            Box::new(move || {
                Box::pin(async move {
                    services.write().await.remove(&drop_key);
                    Ok(())
                })
            }),
        )
        .await;
        Ok(())
    }

    /// Removes the service registered by `owner` for `T`/`qualifier` and
    /// prunes its disposer (the removal IS the disposer's work).
    pub async fn unregister_service<T: Any + Send + Sync>(
        &self,
        owner: &str,
        qualifier: &str,
    ) -> Result<(), HarnessError> {
        let key = (TypeId::of::<T>(), qualifier.to_string());
        let name = format!("{}::{qualifier}", std::any::type_name::<T>());
        let tag = format!("service:{}::{qualifier}", std::any::type_name::<T>());
        {
            let mut map = self.services.write().await;
            match map.get(&key) {
                None => return Err(HarnessError::ServiceNotFound { name }),
                Some((registered_owner, _)) if registered_owner != owner => {
                    return Err(HarnessError::DuplicateService {
                        name,
                        owner: registered_owner.clone(),
                    });
                }
                Some(_) => {
                    map.remove(&key);
                }
            }
        }
        self.prune_effects(owner, &tag).await;
        Ok(())
    }

    /// Resolves the service registered for `T` with `qualifier`.
    pub async fn service<T: Any + Send + Sync>(&self, qualifier: &str) -> Option<Arc<T>> {
        let key = (TypeId::of::<T>(), qualifier.to_string());
        let map = self.services.read().await;
        let (_, boxed) = map.get(&key)?;
        boxed.clone().downcast::<T>().ok()
    }

    /// Registers a reversible side effect owned by `owner`. `tag` correlates
    /// the effect (e.g. for pruning on explicit unregister); duplicate tags
    /// are allowed and disposed/dispatched in LIFO order.
    pub async fn effect(
        &self,
        owner: &str,
        tag: impl Into<String>,
        disposer: Disposer,
    ) -> EffectGuard {
        let tag = tag.into();
        self.effects
            .write()
            .await
            .entry(owner.to_string())
            .or_default()
            .push(Effect {
                tag: tag.clone(),
                disposer,
            });
        EffectGuard {
            owner: owner.to_string(),
            tag,
        }
    }

    /// Runs and removes the single effect identified by `guard`.
    pub async fn dispose_effect(&self, guard: &EffectGuard) -> Result<(), HarnessError> {
        let effect = {
            let mut stacks = self.effects.write().await;
            let Some(stack) = stacks.get_mut(&guard.owner) else {
                return Err(HarnessError::EffectNotFound {
                    owner: guard.owner.clone(),
                    tag: guard.tag.clone(),
                });
            };
            let idx = stack
                .iter()
                .position(|e| e.tag == guard.tag)
                .ok_or_else(|| HarnessError::EffectNotFound {
                    owner: guard.owner.clone(),
                    tag: guard.tag.clone(),
                })?;
            stack.remove(idx)
        };
        (effect.disposer)().await
    }

    /// Runs every disposer registered by `owner` in LIFO order. A failing
    /// disposer never aborts the sweep; its error is aggregated into the
    /// returned vector.
    pub async fn dispose_owner(&self, owner: &str) -> Vec<HarnessError> {
        let stack = {
            let mut stacks = self.effects.write().await;
            stacks.remove(owner).unwrap_or_default()
        };
        let mut errors = Vec::new();
        for effect in stack.into_iter().rev() {
            if let Err(e) = (effect.disposer)().await {
                errors.push(e);
            }
        }
        errors
    }

    /// Drops the effects tagged `tag` for `owner` without running them
    /// (used when the effect's work was already done explicitly).
    async fn prune_effects(&self, owner: &str, tag: &str) {
        let mut stacks = self.effects.write().await;
        if let Some(stack) = stacks.get_mut(owner) {
            stack.retain(|e| e.tag != tag);
        }
    }

    /// Publishes an event on the kernel bus.
    pub fn publish(&self, event: Event) {
        self.bus.publish(event);
    }

    /// Subscribes to the kernel bus.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<Event> {
        self.bus.subscribe()
    }

    /// Registers a waterfall handler; see [`EventBus::on`].
    pub async fn on(
        &self,
        pattern: &str,
        name: impl Into<String>,
        handler: super::WaterfallHandler,
    ) {
        self.bus.on(pattern, name, handler).await;
    }

    /// Runs an event through the waterfall chain; see [`EventBus::waterfall`].
    pub async fn waterfall(&self, event: Event) -> Result<Event, super::EventRejected> {
        self.bus.waterfall(event).await
    }

    /// Clones a handle onto the kernel event bus (cheap: shared broadcast
    /// hub). Needed by components that publish outside the plugin-lifecycle
    /// path, e.g. the WhiteBoard mirror in team runs.
    pub fn bus(&self) -> EventBus {
        self.bus.clone()
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

    #[tokio::test]
    async fn cloned_bus_handle_publishes_to_subscribers() {
        let ctx = Context::default();
        let bus = ctx.bus();
        let mut rx = bus.subscribe();
        bus.publish(Event::new("bus.clone", serde_json::json!({})));
        assert_eq!(rx.recv().await.unwrap().topic, "bus.clone");
    }

    #[tokio::test]
    async fn dispose_owner_runs_disposers_in_lifo_order() {
        let ctx = Context::default();
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        for name in ["one", "two", "three"] {
            let log = Arc::clone(&order);
            ctx.effect(
                "plugin-a",
                name,
                Box::new(move || {
                    Box::pin(async move {
                        log.lock().unwrap().push(name.to_string());
                        Ok(())
                    })
                }),
            )
            .await;
        }
        let errors = ctx.dispose_owner("plugin-a").await;
        assert!(errors.is_empty());
        assert_eq!(*order.lock().unwrap(), vec!["three", "two", "one"]);
        // Second sweep is a no-op.
        assert!(ctx.dispose_owner("plugin-a").await.is_empty());
    }

    #[tokio::test]
    async fn failing_disposer_does_not_abort_sweep() {
        let ctx = Context::default();
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        ctx.effect(
            "p",
            "first",
            Box::new(move || {
                Box::pin(async move {
                    flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    Ok(())
                })
            }),
        )
        .await;
        ctx.effect(
            "p",
            "boom",
            Box::new(|| Box::pin(async { Err(HarnessError::PluginNotFound("boom".into())) })),
        )
        .await;
        let errors = ctx.dispose_owner("p").await;
        assert_eq!(errors.len(), 1);
        assert!(ran.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn unregister_service_removes_entry_and_prunes_disposer() {
        let ctx = Context::default();
        ctx.register_service("a", "q", Arc::new(Greeter { prefix: "1".into() }))
            .await
            .unwrap();
        ctx.unregister_service::<Greeter>("a", "q").await.unwrap();
        assert_eq!(ctx.service_count().await, 0);
        assert!(ctx.service::<Greeter>("q").await.is_none());
        // Wrong owner or missing service errors clearly.
        ctx.register_service("a", "q", Arc::new(Greeter { prefix: "1".into() }))
            .await
            .unwrap();
        assert!(matches!(
            ctx.unregister_service::<Greeter>("b", "q").await,
            Err(HarnessError::DuplicateService { .. })
        ));
        ctx.unregister_service::<Greeter>("a", "q").await.unwrap();
        assert!(matches!(
            ctx.unregister_service::<Greeter>("a", "q").await,
            Err(HarnessError::ServiceNotFound { .. })
        ));
        // dispose_owner afterwards has no service disposer left to run.
        assert!(ctx.dispose_owner("a").await.is_empty());
    }

    #[tokio::test]
    async fn service_disposer_removes_service_on_owner_dispose() {
        let ctx = Context::default();
        ctx.register_service("svc-owner", "", Arc::new(Greeter { prefix: "x".into() }))
            .await
            .unwrap();
        assert!(ctx.service::<Greeter>("").await.is_some());
        assert!(ctx.dispose_owner("svc-owner").await.is_empty());
        assert!(ctx.service::<Greeter>("").await.is_none());
    }

    #[tokio::test]
    async fn dispose_effect_runs_single_effect() {
        let ctx = Context::default();
        let hits = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let log = Arc::clone(&hits);
        let guard = ctx
            .effect(
                "p",
                "solo",
                Box::new(move || {
                    Box::pin(async move {
                        log.lock().unwrap().push("solo".into());
                        Ok(())
                    })
                }),
            )
            .await;
        ctx.effect(
            "p",
            "other",
            Box::new(|| Box::pin(async { Err(HarnessError::PluginNotFound("x".into())) })),
        )
        .await;
        ctx.dispose_effect(&guard).await.unwrap();
        assert_eq!(*hits.lock().unwrap(), vec!["solo"]);
        assert_eq!(ctx.dispose_owner("p").await.len(), 1);
        // Guard is spent: disposing again reports not-found.
        assert!(matches!(
            ctx.dispose_effect(&guard).await,
            Err(HarnessError::EffectNotFound { .. })
        ));
    }
}
