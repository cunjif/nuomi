//! Code sandbox domain (ADR-0022): types, executor contract, task lifecycle.
//!
//! Three layers: the builtin sandbox plugin shell (plugins/sandbox.rs)
//! delegates to the [`SandboxExecutor`] trait (executor.rs) whose default
//! implementation (cube_executor.rs) talks to a CubeSandbox-class runtime.
//! [`SandboxTaskManager`] (task_manager.rs) persists to `sandbox_tasks`
//! (migration 0037) and audits every state transition as a
//! `sandbox.task_state_changed` event BEFORE any external side effect —
//! mirroring the Run state-machine iron rule (AGENTS.md §4).

pub mod cube_executor;
pub mod executor;
pub mod fake;
pub mod task_manager;
pub mod types;

pub use cube_executor::CubeSandboxExecutor;
pub use executor::{SandboxArtifact, SandboxExecutor, SandboxOutputChunk, SandboxTaskSnapshot};
pub use fake::{FakeExecutorCall, FakeSandboxExecutor};
pub use task_manager::{SandboxTaskManager, SubmitCheck, AGGREGATE_TYPE, EVENT_KIND};
pub use types::{
    dirs_from_json, dirs_to_json, SandboxError, SandboxLimits, SandboxResourceUsage,
    SandboxRuntimeInfo, SandboxTaskResult, SandboxTaskSpec, SandboxTaskStatus, SandboxTaskType,
};
