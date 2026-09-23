//! AI-assisted commit message generation service.
//!
//! Stateless orchestrator: collects staged diff + recent commit history,
//! resolves the selected (or default) RoleAgent, calls the LLM provider,
//! and returns a suggested commit message. Does NOT execute `git commit`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::domain::AgentRefKind;
use crate::providers::{ChatRequest, SecretStore};
use crate::services::{
    is_role_ready, materialize_single_role, GitService, ResolvedAgent, RoleOverlay,
    SingleRoleContext,
};
use crate::store::repos::{agent_profiles, roles, settings};
use crate::store::StoreError;
use crate::{CoreError};

/// Setting key for the default RoleAgent used by AI commit generation.
/// Decoupled from `conversation.default_agent` to avoid cross-contamination.
pub const AI_COMMIT_DEFAULT_AGENT_KEY: &str = "ai_commit.default_agent";

/// Maximum diff text length (characters) sent to the AI; longer diffs are
/// hard-truncated and flagged with `truncated = true`.
pub const AI_COMMIT_DIFF_LIMIT: usize = 20000;

/// Timeout (ms) for the AI `complete` call.
pub const AI_COMMIT_TIMEOUT_MS: u64 = 15000;

/// Number of recent commit subjects used as style reference.
pub const AI_COMMIT_HISTORY_LIMIT: u32 = 10;

/// Errors specific to AI commit generation. Business-flow errors use static
/// codes (`ai_commit.*`) for IpcError mapping; infrastructure errors are
/// wrapped via `Core`.
#[derive(Debug, thiserror::Error)]
pub enum AiCommitError {
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("task join error: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("no staged changes")]
    NoStagedChanges,
    #[error("agent unavailable: {0}")]
    AgentUnavailable(String),
    #[error("generation failed: {0}")]
    GenerationFailed(String),
    #[error("AI returned empty result")]
    EmptyResult,
    #[error("generation timed out after {0}ms")]
    Timeout(u64),
}

impl From<crate::services::GitError> for AiCommitError {
    fn from(e: crate::services::GitError) -> Self {
        Self::Core(CoreError::from(e))
    }
}

impl From<StoreError> for AiCommitError {
    fn from(e: StoreError) -> Self {
        Self::Core(CoreError::from(e))
    }
}

/// A selectable RoleAgent option for commit generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitAgentOption {
    pub kind: AgentRefKind,
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// The result of an AI commit generation request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiCommitResult {
    pub message: String,
    pub truncated: bool,
    pub agent_name: String,
    pub elapsed_ms: u64,
}

/// Lists all RoleAgents available for commit generation.
///
/// Source = enabled AgentProfiles ∪ usable Roles (non-ephemeral and ready).
/// The entry matching `ai_commit.default_agent` is flagged `is_default = true`.
pub fn list_commit_agents(conn: &Connection) -> Result<Vec<CommitAgentOption>, StoreError> {
    let mut options = Vec::new();

    let roles_list = roles::list(conn)?;
    for role in roles_list {
        if !role.ephemeral && is_role_ready(&role) {
            options.push(CommitAgentOption {
                kind: AgentRefKind::Role,
                id: role.id,
                name: role.name,
                is_default: false,
            });
        }
    }

    let profiles = agent_profiles::list(conn)?;
    for profile in profiles {
        if profile.enabled {
            options.push(CommitAgentOption {
                kind: AgentRefKind::Cli,
                id: profile.id,
                name: profile.name,
                is_default: false,
            });
        }
    }

    options.sort_by(|a, b| a.name.cmp(&b.name));

    if let Some(default) = settings::get(conn, AI_COMMIT_DEFAULT_AGENT_KEY)? {
        if let Some((kind, id)) = parse_agent_ref(&default) {
            for opt in &mut options {
                if opt.kind == kind && opt.id == id {
                    opt.is_default = true;
                    break;
                }
            }
        }
    }

    Ok(options)
}

/// Generates a commit message from the staged diff using the selected
/// (or default) RoleAgent.
///
/// Orchestrates: staged-check → diff-staged → truncate → log(10) →
/// resolve-agent → materialize → complete(15s timeout) → validate → trace.
pub async fn generate(
    role_agent: Option<(AgentRefKind, String)>,
    db_path: Arc<str>,
    secrets: Arc<dyn SecretStore>,
    cwd: Option<PathBuf>,
    git: &GitService,
) -> Result<AiCommitResult, AiCommitError> {
    let status = git.status().await?;
    let has_staged = status.iter().any(|s| s.index_status != ' ');
    if !has_staged {
        return Err(AiCommitError::NoStagedChanges);
    }

    let diff = git.diff_staged().await?;
    let (diff_content, truncated) = truncate_diff(&diff);

    let history = git.log(AI_COMMIT_HISTORY_LIMIT).await?;
    let history_samples: Vec<&str> = history.iter().map(|c| c.subject.as_str()).collect();

    let resolved = resolve_role_agent(&db_path, role_agent).await?;

    let agent_display_name = resolved.as_ref().map(|r| r.name.clone()).unwrap_or_default();

    let ctx = materialize_single_role(db_path, secrets, cwd, resolved.as_ref()).await?;
    let (provider, model, overlay) = match ctx {
        SingleRoleContext::Materialized {
            provider,
            model,
            overlay,
            ..
        } => (provider, model, overlay),
        SingleRoleContext::EnvFallback => {
            tracing::warn!(
                agent = %agent_display_name,
                "ai_commit: EnvFallback — no provider available"
            );
            return Err(AiCommitError::AgentUnavailable(
                "no provider configured for the selected agent".into(),
            ));
        }
    };

    let system_prompt = build_system_prompt(overlay.as_ref());
    let user_prompt = build_user_prompt(&diff_content, &history_samples);
    let mut req = ChatRequest::simple(&model, &system_prompt, &user_prompt);
    if let Some(ref ov) = overlay {
        if let Some(t) = ov.temperature {
            req.temperature = Some(t);
        }
    }

    let start = Instant::now();
    let outcome = tokio::time::timeout(
        Duration::from_millis(AI_COMMIT_TIMEOUT_MS),
        provider.complete(&req),
    )
    .await;

    match outcome {
        Err(_) => {
            tracing::warn!(
                agent = %agent_display_name,
                diff_chars = diff.len(),
                truncated,
                timeout_ms = AI_COMMIT_TIMEOUT_MS,
                "ai_commit: timed out"
            );
            Err(AiCommitError::Timeout(AI_COMMIT_TIMEOUT_MS))
        }
        Ok(Err(e)) => {
            tracing::warn!(
                agent = %agent_display_name,
                diff_chars = diff.len(),
                truncated,
                error = %e,
                "ai_commit: provider error"
            );
            Err(AiCommitError::GenerationFailed(e.to_string()))
        }
        Ok(Ok(response)) => {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let message = response.content.trim().to_string();
            if message.is_empty() {
                tracing::warn!(
                    agent = %agent_display_name,
                    diff_chars = diff.len(),
                    truncated,
                    elapsed_ms,
                    "ai_commit: empty result"
                );
                return Err(AiCommitError::EmptyResult);
            }

            tracing::info!(
                agent = %agent_display_name,
                diff_chars = diff.len(),
                truncated,
                elapsed_ms,
                "ai_commit: generation succeeded"
            );

            Ok(AiCommitResult {
                message,
                truncated,
                agent_name: agent_display_name,
                elapsed_ms,
            })
        }
    }
}

// ---- helpers ----

fn truncate_diff(diff: &str) -> (String, bool) {
    if diff.chars().count() > AI_COMMIT_DIFF_LIMIT {
        let truncated: String = diff.chars().take(AI_COMMIT_DIFF_LIMIT).collect();
        (truncated, true)
    } else {
        (diff.to_string(), false)
    }
}

fn build_system_prompt(overlay: Option<&RoleOverlay>) -> String {
    let base = "You are a commit message generator. Based on the staged diff \
                and recent commit history, generate a concise commit message \
                following the team's conventions (type prefix + concise subject \
                line + optional body). Output ONLY the commit message itself, \
                nothing else.";

    if let Some(ov) = overlay {
        if let Some(ref role_prompt) = ov.system_prompt {
            return format!("{role_prompt}\n\n{base}");
        }
    }
    base.to_string()
}

fn build_user_prompt(diff: &str, history: &[&str]) -> String {
    let mut prompt = String::new();

    if !history.is_empty() {
        prompt.push_str("Recent commit messages (for style reference):\n");
        for subject in history {
            prompt.push_str("  - ");
            prompt.push_str(subject);
            prompt.push('\n');
        }
        prompt.push('\n');
    }

    prompt.push_str("Staged diff:\n");
    prompt.push_str(diff);

    prompt
}

fn parse_agent_ref(s: &str) -> Option<(AgentRefKind, String)> {
    if let Some(id) = s.strip_prefix("cli:") {
        Some((AgentRefKind::Cli, id.to_string()))
    } else if let Some(id) = s.strip_prefix("role:") {
        Some((AgentRefKind::Role, id.to_string()))
    } else {
        None
    }
}

async fn resolve_role_agent(
    db_path: &Arc<str>,
    role_agent: Option<(AgentRefKind, String)>,
) -> Result<Option<ResolvedAgent>, AiCommitError> {
    let db_path = db_path.clone();
    tokio::task::spawn_blocking(move || -> Result<Option<ResolvedAgent>, StoreError> {
        let db = crate::store::Db::open(&db_path)?;
        crate::store::migrations::run(&db.0)?;
        let conn = &db.0;

        if let Some((kind, id)) = role_agent {
            return Ok(resolve_ref(conn, kind, &id));
        }

        if let Some(default) = settings::get(conn, AI_COMMIT_DEFAULT_AGENT_KEY)? {
            if let Some((kind, id)) = parse_agent_ref(&default) {
                return Ok(resolve_ref(conn, kind, &id));
            }
        }

        Ok(None)
    })
    .await?
    .map_err(AiCommitError::from)
}

fn resolve_ref(conn: &Connection, kind: AgentRefKind, id: &str) -> Option<ResolvedAgent> {
    match kind {
        AgentRefKind::Cli => match agent_profiles::get(conn, id) {
            Ok(profile) if profile.enabled => Some(ResolvedAgent {
                kind: AgentRefKind::Cli,
                id: profile.id,
                name: profile.name,
            }),
            _ => None,
        },
        AgentRefKind::Role => match roles::get(conn, id) {
            Ok(role) if !role.ephemeral && is_role_ready(&role) => Some(ResolvedAgent {
                kind: AgentRefKind::Role,
                id: role.id,
                name: role.name,
            }),
            _ => None,
        },
    }
}

/// Sets the default RoleAgent for commit generation (`ai_commit.default_agent`).
pub fn set_default_agent(conn: &Connection, kind: AgentRefKind, id: &str) -> Result<(), StoreError> {
    let value = format!("{}:{id}", kind.as_str());
    settings::set(conn, AI_COMMIT_DEFAULT_AGENT_KEY, &value)
}

/// Reads the default RoleAgent setting for commit generation.
pub fn get_default_agent(conn: &Connection) -> Result<Option<(AgentRefKind, String)>, StoreError> {
    let value = settings::get(conn, AI_COMMIT_DEFAULT_AGENT_KEY)?;
    Ok(value.as_deref().and_then(parse_agent_ref))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_diff_under_limit() {
        let diff = "short diff";
        let (result, truncated) = truncate_diff(diff);
        assert_eq!(result, diff);
        assert!(!truncated);
    }

    #[test]
    fn truncate_diff_over_limit() {
        let diff: String = "x".repeat(AI_COMMIT_DIFF_LIMIT + 100);
        let (result, truncated) = truncate_diff(&diff);
        assert_eq!(result.chars().count(), AI_COMMIT_DIFF_LIMIT);
        assert!(truncated);
    }

    #[test]
    fn truncate_diff_at_exact_limit() {
        let diff: String = "x".repeat(AI_COMMIT_DIFF_LIMIT);
        let (result, truncated) = truncate_diff(&diff);
        assert_eq!(result.chars().count(), AI_COMMIT_DIFF_LIMIT);
        assert!(!truncated);
    }

    #[test]
    fn parse_agent_ref_cli() {
        let result = parse_agent_ref("cli:abc123");
        assert_eq!(result, Some((AgentRefKind::Cli, "abc123".to_string())));
    }

    #[test]
    fn parse_agent_ref_role() {
        let result = parse_agent_ref("role:def456");
        assert_eq!(result, Some((AgentRefKind::Role, "def456".to_string())));
    }

    #[test]
    fn parse_agent_ref_invalid() {
        assert_eq!(parse_agent_ref("invalid"), None);
        assert_eq!(parse_agent_ref(""), None);
    }

    #[test]
    fn build_system_prompt_without_overlay() {
        let prompt = build_system_prompt(None);
        assert!(prompt.contains("commit message generator"));
    }

    #[test]
    fn build_system_prompt_with_overlay() {
        let overlay = RoleOverlay {
            system_prompt: Some("You are a senior Rust engineer.".into()),
            temperature: None,
            tool_allowlist: Vec::new(),
        };
        let prompt = build_system_prompt(Some(&overlay));
        assert!(prompt.starts_with("You are a senior Rust engineer."));
        assert!(prompt.contains("commit message generator"));
    }

    #[test]
    fn build_user_prompt_with_history() {
        let history = vec!["feat: add login", "fix: crash on startup"];
        let prompt = build_user_prompt("diff content", &history);
        assert!(prompt.contains("Recent commit messages"));
        assert!(prompt.contains("feat: add login"));
        assert!(prompt.contains("fix: crash on startup"));
        assert!(prompt.contains("Staged diff:"));
        assert!(prompt.contains("diff content"));
    }

    #[test]
    fn build_user_prompt_without_history() {
        let prompt = build_user_prompt("diff content", &[]);
        assert!(!prompt.contains("Recent commit messages"));
        assert!(prompt.contains("Staged diff:"));
        assert!(prompt.contains("diff content"));
    }
}
