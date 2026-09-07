//! Built-in preset catalog: cross-domain Role presets seeded on first
//! launch / "restore presets" (idempotent by `name`).
//!
//! Role taxonomy distilled from public harness designs — OpenCode's
//! build/plan/general agent split, KiloCode's mode set (architect / code /
//! debug / ask / orchestrator), Claude Code's planner + code-reviewer
//! subagents and MetaGPT/AutoGen's Planner-Coder-Reviewer-Tester pipeline
//! (see docs/specs/harness-research-synthesis.md) — extended with the
//! nuomi-specific meta role "Role Director".

pub mod roles;

pub use roles::{seed_builtin_roles, PresetRole, SeedReport, PRESET_ROLES};
