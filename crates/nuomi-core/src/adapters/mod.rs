//! External agent adapters (SPEC: docs/specs/cli-agents-m1.md).
//!
//! Adapters wrap non-HTTP agents behind the kernel-facing
//! [`crate::providers::LlmProvider`] trait so they can join Teams as
//! first-class members with zero orchestrator changes. The v1 adapter is
//! `cli`: external CLI agent processes (Claude Code / Codex / plain-text
//! scripts) spawned safely — arg arrays only, executable allowlist,
//! `kill_on_drop` reaping.

pub mod cli;
pub mod pty;
pub mod traits;

pub use cli::CliAgentClient;
pub use pty::{
    strip_ansi, DeliveryReadiness, PromptAnchor, PromptDetector, PromptPattern, PtySession,
    ReadinessPoller, PROMPT_PATTERNS,
};
pub use traits::{
    AdapterCapabilities, AgentAdapter, BinaryResolver, EnvSanitizer, ResumableSession,
};

use thiserror::Error;

/// Errors produced by the adapters layer.
#[derive(Debug, Error)]
pub enum AdapterError {
    /// The profile's executable is not on the caller-supplied allowlist.
    #[error("command '{command}' is not allowlisted for cli agents")]
    CommandNotAllowlisted { command: String },

    /// The child process failed to start.
    #[error("failed to spawn '{command}': {source}")]
    Spawn {
        command: String,
        source: std::io::Error,
    },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The agent violated its flavor protocol (or config was malformed).
    #[error("protocol violation from '{agent}': {message}")]
    Protocol { agent: String, message: String },
}

/// Candidate program names to try when spawning `program`.
///
/// On Windows a bare name like `codebuddy` may resolve — via the standard
/// library's PATH search — to an extensionless `#!/bin/sh` shim that
/// `CreateProcess` cannot execute. npm/pnpm global installs produce exactly
/// this trio: `codebuddy` (sh script), `codebuddy.cmd` (cmd wrapper),
/// `codebuddy.ps1` (PowerShell). The extensionless file sorts first in Rust's
/// PATH search, so `Command::new("codebuddy").spawn()` fails with "program
/// not found" / "not a valid Win32 application".
///
/// Returns `[program]` plus, on Windows when `program` is a bare name (no
/// extension, no path separator), the PATHEXT variants `.cmd`, `.bat`, `.exe`,
/// `.com` — letting the caller retry spawn on failure without duplicating the
/// PATH search itself. Non-Windows returns `[program]`. Names that already
/// carry an extension or a path separator are returned as-is on every platform.
pub fn spawn_candidates(program: &str) -> Vec<String> {
    let mut out = vec![program.to_string()];
    if cfg!(windows) {
        let bare = std::path::Path::new(program)
            .extension()
            .is_none()
            && !program.contains('/')
            && !program.contains('\\');
        if bare {
            for ext in [".cmd", ".bat", ".exe", ".com"] {
                out.push(format!("{program}{ext}"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::spawn_candidates;

    #[test]
    fn bare_name_returns_self_first() {
        let c = spawn_candidates("codebuddy");
        assert_eq!(c.first().map(String::as_str), Some("codebuddy"));
    }

    #[test]
    fn name_with_extension_is_returned_as_is() {
        let c = spawn_candidates("codebuddy.cmd");
        assert_eq!(c, vec!["codebuddy.cmd".to_string()]);
    }

    #[test]
    fn name_with_path_separator_is_returned_as_is() {
        let c = spawn_candidates("C:\\dir\\codebuddy");
        assert_eq!(c, vec!["C:\\dir\\codebuddy".to_string()]);
        let c2 = spawn_candidates("/usr/bin/node");
        assert_eq!(c2, vec!["/usr/bin/node".to_string()]);
    }

    #[cfg(windows)]
    #[test]
    fn bare_name_appends_pathext_variants_on_windows() {
        let c = spawn_candidates("codebuddy");
        assert_eq!(
            c,
            vec![
                "codebuddy".to_string(),
                "codebuddy.cmd".to_string(),
                "codebuddy.bat".to_string(),
                "codebuddy.exe".to_string(),
                "codebuddy.com".to_string(),
            ]
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn bare_name_no_extra_variants_on_non_windows() {
        let c = spawn_candidates("codebuddy");
        assert_eq!(c, vec!["codebuddy".to_string()]);
    }
}
