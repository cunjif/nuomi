//! nuomi-core: the nuomi agent harness kernel.
//!
//! Module map (see AGENTS.md §3 and docs/adr/0001-rust-plugin-kernel.md):
//! - `domain`: entities + run state machine (single source of truth)
//! - `harness`: plugin kernel (plugin registry, context, event bus)
//! - `adapters`: external agent adapters (cli agents) as LlmProvider
//! - `providers`: provider clients + master-slave orchestration
//! - `orchestrator`: role/team executors (pipeline, router, group chat, whiteboard)
//! - `services`: UI-M1 domain services (workspace sandbox, git, scheduler)
//! - `store`: SQLite repositories + migrations runner
//! - `evolution`: self-evolution (trajectory aggregation, prompt versioning, research)

pub mod adapters;
pub mod domain;
pub mod error;
pub mod evolution;
pub mod facade;
pub mod harness;
pub mod orchestrator;
pub mod plugins;
pub mod providers;
pub mod services;
pub mod store;

pub use error::{CoreError, CoreResult};
