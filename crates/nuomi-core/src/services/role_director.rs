//! Role Director service: turns a plain-language description ("我要一个帮我
//! 写周报的角色") into a validated, persisted [`Role`].
//!
//! Flow (mirrors [`super::team_former`]'s discipline — validate before any
//! write): materialize the default provider → one LLM round-trip with a
//! strict JSON schema prompt (one parse-failure retry) → schema/semantic
//! validation → insert with `generated = true` and `source` recording the
//! generation parameters (description, provider, model, timestamp).

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::domain::{new_id, now_ms, Capability, Role};
use crate::providers::{ChatRequest, LlmProvider, SecretStore};
use crate::store::{migrations, repos, Db};

/// Everything that can fail on the way to a generated role.
#[derive(Debug, thiserror::Error)]
pub enum RoleDirectorError {
    #[error("no provider available for role generation")]
    NoProvider,
    #[error("role director has no self-binding configured")]
    NoBinding,
    #[error("generated role rejected: {0}")]
    Rejected(String),
    #[error("store: {0}")]
    Store(String),
    #[error("provider call failed: {0}")]
    Provider(String),
}

/// Whether the role director binds to a Provider config or a CLI Agent profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RoleDirectorBindingMode {
    Provider,
    Cli,
}

/// Self-binding of the role director: the model the director itself uses to
/// orchestrate / generate roles. This is NOT inherited by the roles it
/// generates — those keep `provider_id: None` and must be bound separately
/// in the Roles panel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoleDirectorBinding {
    pub binding_mode: RoleDirectorBindingMode,
    pub provider_id: Option<String>,
    pub agent_profile_id: Option<String>,
}

impl RoleDirectorBinding {
    /// Returns the id used to look up the materialized provider —
    /// `provider_id` for Provider mode, `agent_profile_id` for Cli mode.
    fn target_id(&self) -> Option<&str> {
        match self.binding_mode {
            RoleDirectorBindingMode::Provider => self.provider_id.as_deref(),
            RoleDirectorBindingMode::Cli => self.agent_profile_id.as_deref(),
        }
    }
}

/// app_settings key under which the director self-binding is JSON-persisted.
const ROLE_DIRECTOR_BINDING_KEY: &str = "role_director_binding";

/// Reads the persisted director self-binding, or `None` when unset.
pub fn get_role_director_binding(
    conn: &rusqlite::Connection,
) -> Result<Option<RoleDirectorBinding>, RoleDirectorError> {
    let raw = repos::settings::get(conn, ROLE_DIRECTOR_BINDING_KEY)
        .map_err(|e| RoleDirectorError::Store(e.to_string()))?;
    match raw {
        None => Ok(None),
        Some(json) => serde_json::from_str(&json)
            .map(Some)
            .map_err(|e| RoleDirectorError::Store(format!("invalid binding json: {e}"))),
    }
}

/// Persists the director self-binding as JSON under `role_director_binding`.
pub fn set_role_director_binding(
    conn: &rusqlite::Connection,
    binding: &RoleDirectorBinding,
) -> Result<(), RoleDirectorError> {
    let json = serde_json::to_string(binding)
        .map_err(|e| RoleDirectorError::Store(format!("binding serialize: {e}")))?;
    repos::settings::set(conn, ROLE_DIRECTOR_BINDING_KEY, &json)
        .map_err(|e| RoleDirectorError::Store(e.to_string()))
}

/// A validated, not-yet-persisted generated role. Wire format is camelCase
/// (the director prompt's JSON schema: `systemPrompt`, `requiredCapabilities`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedRoleDraft {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
}

const DIRECTOR_SYSTEM_PROMPT: &str = "You are the Role Director of an agent harness. \
The user describes, in any language, the assistant they need. Design exactly one executable \
Role for it. Respond with ONLY one JSON object - no markdown fences, no commentary. Schema:\n\
{\"name\":string,\"description\":string,\"systemPrompt\":string,\"requiredCapabilities\":[string]}\n\
Rules:\n\
- name: 2-40 chars, concise, unique-sounding, may be Chinese or English.\n\
- description: one sentence, what the role is for.\n\
- systemPrompt: 3+ sentences encoding expertise, working style, output format and guardrails.\n\
- requiredCapabilities: subset of [\"reasoning\",\"image\",\"voice\",\"video\"] the role needs \
from its model provider; most roles need only [\"reasoning\"].";

/// Generates and persists a role for `description` using the director's
/// **self-binding** — the Provider or CLI Agent the director itself uses to
/// orchestrate. The binding is NOT inherited by the generated role (it keeps
/// `provider_id: None`); bind the role separately in the Roles panel.
pub async fn generate_role(
    db_path: Arc<str>,
    secrets: Arc<dyn SecretStore>,
    cwd: Option<PathBuf>,
    description: &str,
    binding: RoleDirectorBinding,
) -> Result<Role, RoleDirectorError> {
    let description = description.trim();
    if description.is_empty() {
        return Err(RoleDirectorError::Rejected(
            "description must not be empty".to_string(),
        ));
    }

    let materialized = super::team_runner::materialize(db_path.clone(), secrets, cwd)
        .await
        .map_err(|e| RoleDirectorError::Store(e.to_string()))?;
    // Resolve the director's own bound model — never fall back to the default
    // provider. A missing target id or a deleted provider/agent → NoBinding.
    let target_id = binding.target_id().ok_or(RoleDirectorError::NoBinding)?;
    let provider = materialized
        .providers
        .get(target_id)
        .cloned()
        .ok_or(RoleDirectorError::NoBinding)?;
    let model = provider.id().to_string();

    let request = ChatRequest::simple(
        &model,
        DIRECTOR_SYSTEM_PROMPT,
        &format!("Desired assistant:\n{description}"),
    );
    let raw = match ask(&provider, &request).await {
        Ok(raw) => raw,
        Err(first) => {
            tracing::warn!(error = %first, "role director first call failed; retrying once");
            ask(&provider, &request).await?
        }
    };

    let draft = parse_draft(&raw)?;
    validate_draft(&draft)?;

    let role = draft.into_role(description, &model);
    persist(db_path, role).await
}

impl GeneratedRoleDraft {
    /// Projects a validated draft onto a `Role` row with generation
    /// provenance recorded in `source`.
    fn into_role(self, description: &str, model: &str) -> Role {
        Role {
            id: new_id(),
            name: self.name.trim().to_string(),
            provider_id: None,
            provider_ids: vec![],
            system_prompt_override: Some(self.system_prompt),
            tool_allowlist: vec![],
            required_capabilities: self
                .required_capabilities
                .iter()
                .filter_map(|c| Capability::parse(c.trim()))
                .collect(),
            temperature: None,
            max_tokens: None,
            params: serde_json::json!({ "description": self.description.trim() }),
            builtin: false,
            generated: true,
            ephemeral: false,
            source: Some(serde_json::json!({
                "description": description,
                "model": model,
                "generatedAt": now_ms(),
            })),
            created_at: now_ms(),
            updated_at: now_ms(),
        }
    }
}

async fn ask(
    provider: &Arc<dyn LlmProvider>,
    request: &ChatRequest,
) -> Result<String, RoleDirectorError> {
    let response = provider
        .complete(request)
        .await
        .map_err(|e| RoleDirectorError::Provider(e.to_string()))?;
    Ok(response.content)
}

/// Parses the model output: strips optional ``` fences, then deserializes.
pub(crate) fn parse_draft(raw: &str) -> Result<GeneratedRoleDraft, RoleDirectorError> {
    let stripped = strip_code_fences(raw);
    serde_json::from_str(stripped)
        .map_err(|e| RoleDirectorError::Rejected(format!("not valid JSON: {e}")))
}

/// Semantic validation applied before persistence.
pub(crate) fn validate_draft(draft: &GeneratedRoleDraft) -> Result<(), RoleDirectorError> {
    let name = draft.name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err(RoleDirectorError::Rejected(format!(
            "name must be 1..=40 chars, got {name:?}"
        )));
    }
    if draft.system_prompt.trim().is_empty() {
        return Err(RoleDirectorError::Rejected(
            "systemPrompt must not be empty".to_string(),
        ));
    }
    if draft.description.trim().is_empty() {
        return Err(RoleDirectorError::Rejected(
            "description must not be empty".to_string(),
        ));
    }
    for cap in &draft.required_capabilities {
        if Capability::parse(cap.trim()).is_none() {
            return Err(RoleDirectorError::Rejected(format!(
                "unknown capability {cap:?}; allowed: reasoning|image|voice|video"
            )));
        }
    }
    Ok(())
}

fn strip_code_fences(raw: &str) -> &str {
    let trimmed = raw.trim();
    let body = match trimmed.strip_prefix("```") {
        Some(rest) => match rest.find('\n') {
            Some(newline) => &rest[newline + 1..],
            None => rest,
        },
        None => trimmed,
    };
    body.strip_suffix("```").unwrap_or(body).trim()
}

/// Uniquifies the name against the live table, then inserts. Returns the
/// persisted row (with its final name).
async fn persist(db_path: Arc<str>, role: Role) -> Result<Role, RoleDirectorError> {
    tokio::task::spawn_blocking(move || -> Result<Role, RoleDirectorError> {
        let db = Db::open(&db_path).map_err(|e| RoleDirectorError::Store(e.to_string()))?;
        migrations::run(&db.0).map_err(|e| RoleDirectorError::Store(e.to_string()))?;
        let mut role = role;
        let taken: Vec<String> = repos::roles::list(&db.0)
            .map_err(|e| RoleDirectorError::Store(e.to_string()))?
            .into_iter()
            .map(|r| r.name)
            .collect();
        if taken.iter().any(|n| n.eq_ignore_ascii_case(&role.name)) {
            role.name = uniquify(&role.name, taken.len());
        }
        role.updated_at = now_ms();
        repos::roles::insert(&db.0, &role).map_err(|e| RoleDirectorError::Store(e.to_string()))?;
        Ok(role)
    })
    .await
    .map_err(|e| RoleDirectorError::Store(e.to_string()))?
}

/// `base-<n>` with an increasing n derived from the collision count; good
/// enough for a single director call (races fall back to the UNIQUE error).
fn uniquify(base: &str, attempt: usize) -> String {
    format!("{base}-{}", attempt + 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_draft_accepts_plain_and_fenced_json() {
        let body = r#"{"name":"周报助手","description":"写周报",
            "systemPrompt":"你是周报专家。","requiredCapabilities":["reasoning"]}"#;
        let draft = parse_draft(body).unwrap();
        assert_eq!(draft.name, "周报助手");
        let fenced = format!("```json\n{body}\n```");
        assert_eq!(parse_draft(&fenced).unwrap().name, "周报助手");
        assert!(parse_draft("not json").is_err());
    }

    #[test]
    fn validate_rejects_bad_names_and_unknown_caps() {
        let valid = GeneratedRoleDraft {
            name: "Weekly Reporter".into(),
            description: "writes reports".into(),
            system_prompt: "you write".into(),
            required_capabilities: vec!["reasoning".into()],
        };
        assert!(validate_draft(&valid).is_ok());

        let empty_name = GeneratedRoleDraft {
            name: "  ".into(),
            ..valid.clone()
        };
        assert!(validate_draft(&empty_name).is_err());

        let long_name = GeneratedRoleDraft {
            name: "x".repeat(41),
            ..valid.clone()
        };
        assert!(validate_draft(&long_name).is_err());

        let bad_cap = GeneratedRoleDraft {
            required_capabilities: vec!["telepathy".into()],
            ..valid.clone()
        };
        assert!(validate_draft(&bad_cap).is_err());

        let empty_prompt = GeneratedRoleDraft {
            system_prompt: "  ".into(),
            ..valid
        };
        assert!(validate_draft(&empty_prompt).is_err());
    }

    #[test]
    fn into_role_records_generation_source() {
        let draft = GeneratedRoleDraft {
            name: "Reporter".into(),
            description: "weekly report writer".into(),
            system_prompt: "write well".into(),
            required_capabilities: vec!["reasoning".into(), "bogus".into()],
        };
        validate_draft(&draft).unwrap_err(); // bogus cap — draft used raw below
        let role = draft.into_role("帮我写周报", "openai_compatible");
        assert!(role.generated);
        assert!(!role.builtin);
        assert!(!role.ephemeral);
        // Unknown capability strings are dropped, not fatal at projection.
        assert_eq!(role.required_capabilities, vec![Capability::Reasoning]);
        let source = role.source.unwrap();
        assert_eq!(source["description"], "帮我写周报");
        assert_eq!(source["model"], "openai_compatible");
    }
}
