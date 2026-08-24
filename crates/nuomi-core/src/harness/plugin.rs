//! The `Plugin` trait: the unit of composition for every harness component.

use async_trait::async_trait;

use super::{Context, HarnessError};

/// A harness component. Loop Engine, SystemPrompt, Memory, MCP and Hook are
/// all plugins; adding a new component must not require kernel changes.
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Stable unique id, e.g. `"memory"`, `"mcp"`.
    fn id(&self) -> &str;

    /// Registers services / subscribes to events. Called in registration order.
    async fn init(&self, ctx: &Context) -> Result<(), HarnessError>;

    /// Post-init activation (spawn tasks etc.). Called after all plugins init.
    async fn start(&self) -> Result<(), HarnessError> {
        Ok(())
    }

    /// Reverse-order teardown.
    async fn dispose(&self) -> Result<(), HarnessError> {
        Ok(())
    }
}
