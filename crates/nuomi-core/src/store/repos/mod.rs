//! Repositories: all SQL lives here (parameterized only).

pub mod agent_profiles;
pub mod attachments;
pub mod change_log;
pub mod events;
pub mod integrations;
pub mod journal;
pub mod memory;
pub mod prompts;
pub mod providers;
pub mod roles;
pub mod sessions;
pub mod settings;
pub mod tasks_runs;
pub mod teams;
pub mod whiteboard;
pub mod workspaces;

pub use agent_profiles::{count, delete, get, insert, list, update};
