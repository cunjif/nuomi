//! Harness components implemented as plugins (SPEC D9):
//! Loop Engine (ReAct), SystemPrompt, Memory, Hook, MCP.

pub mod hooks;
pub mod loop_engine;
pub mod mcp;
pub mod memory;
pub mod system_prompt;
pub mod tools;

pub use hooks::{HookDecision, HookPoint, HookRegistry};
pub use loop_engine::{DeltaCallback, LoopConfig, LoopEngine, LoopRunResult};
pub use memory::MemoryService;
pub use system_prompt::SystemPromptService;
pub use tools::ToolRegistry;
