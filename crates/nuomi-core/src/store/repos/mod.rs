//! Repositories: all SQL lives here (parameterized only).

pub mod agent_profiles;
pub mod attachments;
pub mod change_log;
pub mod events;
pub mod integrations;
pub mod journal;
pub mod memory;
pub mod message_queue;
pub mod prompts;
pub mod providers;
pub mod roles;
pub mod session_cli_handles;
pub mod sessions;
pub mod settings;
pub mod tasks_runs;
pub mod teams;
pub mod whiteboard;
pub mod workspaces;
pub mod workspace_open_state;
pub mod workspace_layout_snapshot;
pub mod workspace_recent;

pub use agent_profiles::{count, delete, get, insert, list, update};
