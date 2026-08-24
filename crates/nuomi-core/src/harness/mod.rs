//! Plugin kernel: plugin registry, context, event bus.
//!
//! Cordis-inspired design; see docs/adr/0001-rust-plugin-kernel.md.
//!
//! Lifecycle: plugins registered on a [`Kernel`] are `init`ed in registration
//! order, then `start`ed in the same order; shutdown disposes in reverse order.

pub mod bus;
pub mod context;
pub mod kernel;
pub mod plugin;

pub use bus::{Event, EventBus};
pub use context::Context;
pub use kernel::Kernel;
pub use plugin::Plugin;

use thiserror::Error;

/// Errors produced by the plugin kernel.
#[derive(Debug, Error)]
pub enum HarnessError {
    #[error("plugin '{0}' not registered")]
    PluginNotFound(String),

    #[error("service '{name}' already registered by '{owner}'")]
    DuplicateService { name: String, owner: String },

    #[error("plugin '{plugin}' failed during {phase}: {message}")]
    PluginFailed {
        plugin: String,
        phase: &'static str,
        message: String,
    },
}
