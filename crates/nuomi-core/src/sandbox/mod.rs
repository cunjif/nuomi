//! Code sandbox domain (ADR-0022): types + executor contract. The task
//! lifecycle manager (task_manager.rs) lands with task 7.3.
//!
//! Three layers: the builtin sandbox plugin shell (plugins/sandbox.rs)
//! delegates to the [`SandboxExecutor`] trait (executor.rs) whose default
//! implementation (cube_executor.rs) talks to a CubeSandbox-class runtime.
//! Task lifecycle management persists to `sandbox_tasks` (migration 0037)
//! and audits every state transition as a `sandbox.task_state_changed`
//! event BEFORE any external side effect — mirroring the Run state-machine
//! iron rule (AGENTS.md §4).

pub mod executor;
pub mod fake;
pub mod types;

pub use executor::{SandboxArtifact, SandboxExecutor, SandboxOutputChunk, SandboxTaskSnapshot};
pub use fake::{FakeExecutorCall, FakeSandboxExecutor};
pub use types::{
    dirs_from_json, dirs_to_json, SandboxError, SandboxLimits, SandboxResourceUsage,
    SandboxRuntimeInfo, SandboxTaskResult, SandboxTaskSpec, SandboxTaskStatus, SandboxTaskType,
};
