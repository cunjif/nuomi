//! Hook points: interception around agent actions with audit events.

use std::collections::BTreeMap;

use async_trait::async_trait;

use crate::harness::{Event, HarnessError};

/// Where a hook can fire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HookPoint {
    PreToolCall,
    PostToolCall,
    SessionStart,
    SessionEnd,
}

/// A hook's verdict on a `PreToolCall`.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum HookDecision {
    #[default]
    Allow,
    Deny(String),
}

type HookFn = dyn Fn(&serde_json::Value) -> futures::future::BoxFuture<HookDecision> + Send + Sync;

/// Ordered registry: hooks fire in registration order per point.
#[derive(Default)]
pub struct HookRegistry {
    hooks: tokio::sync::RwLock<BTreeMap<(HookPoint, u32), Box<HookFn>>>,
    next_seq: std::sync::atomic::AtomicU32,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a hook; lower `order` runs earlier (ties by registration).
    pub async fn add<F>(&self, point: HookPoint, order: u32, f: F)
    where
        F: Fn(&serde_json::Value) -> futures::future::BoxFuture<HookDecision>
            + Send
            + Sync
            + 'static,
    {
        let seq = self
            .next_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.hooks
            .write()
            .await
            .insert((point, order * 1_000_000 + seq), Box::new(f));
    }

    /// Runs all hooks at `point` with `payload`; returns the first Deny.
    pub async fn run(&self, point: HookPoint, payload: &serde_json::Value) -> HookDecision {
        let hooks = self.hooks.read().await;
        for ((p, _), f) in hooks.iter() {
            if *p != point {
                continue;
            }
            match f(payload).await {
                HookDecision::Deny(reason) => return HookDecision::Deny(reason),
                HookDecision::Allow => {}
            }
        }
        HookDecision::Allow
    }

    /// Audit event for observability (persisted by the caller).
    pub fn audit_event(
        point: HookPoint,
        payload: &serde_json::Value,
        decision: &HookDecision,
    ) -> Event {
        Event::new(
            format!("hook.{}", point_name(point)),
            serde_json::json!({
                "payload": payload,
                "decision": match decision {
                    HookDecision::Allow => "allow",
                    HookDecision::Deny(_) => "deny",
                },
                "reason": if let HookDecision::Deny(r) = decision { Some(r.clone()) } else { None },
            }),
        )
    }
}

fn point_name(p: HookPoint) -> &'static str {
    match p {
        HookPoint::PreToolCall => "pre_tool_call",
        HookPoint::PostToolCall => "post_tool_call",
        HookPoint::SessionStart => "session_start",
        HookPoint::SessionEnd => "session_end",
    }
}

/// The plugin wrapper registering the shared registry into the Context.
pub struct HooksPlugin {
    registry: std::sync::Arc<HookRegistry>,
}

impl HooksPlugin {
    pub fn new(registry: std::sync::Arc<HookRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl crate::harness::Plugin for HooksPlugin {
    fn id(&self) -> &str {
        "hooks"
    }

    async fn init(&self, ctx: &crate::harness::Context) -> Result<(), HarnessError> {
        ctx.register_service("hooks", "", self.registry.clone())
            .await
    }
}
