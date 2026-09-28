//! Harness components implemented as plugins (SPEC D9):
//! Loop Engine (ReAct), SystemPrompt, Memory, Hook, MCP, Approval Gate.

pub mod approval_gate;
pub mod hooks;
pub mod loop_engine;
pub mod mcp;
pub mod memory;
pub mod system_prompt;
pub mod tools;

pub use approval_gate::{
    execute_with_gate, read_sensitive_tools, resolve_approval, set_sensitive_tools, ApprovalGate,
    GateDecision, SensitiveToolPolicy, SENSITIVE_TOOLS_MARKER,
};
pub use hooks::{HookDecision, HookPoint, HookRegistry, HooksPlugin};
pub use loop_engine::{
    DeltaCallback, LoopConfig, LoopEngine, LoopRunResult, TurnHook, TurnOverride,
};
pub use memory::{MemoryPlugin, MemoryService};
pub use system_prompt::{SystemPromptPlugin, SystemPromptService};
pub use tools::{ToolRegistry, ToolsPlugin};
