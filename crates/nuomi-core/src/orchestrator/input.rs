//! Shared inputs for orchestrator executors: team run input + provider resolution.

use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::{Role, Team};
use crate::providers::{ChatResponse, LlmProvider};

/// A CLI Agent session handle observed during a team run (ADR 0012 D8): the
/// member role that spoke, the agent profile it was bound to, and the CLI
/// session id its client reported. team_runner upserts these into
/// `session_cli_handles` at run settlement so later runs resume each
/// member's own CLI session.
#[derive(Debug, Clone)]
pub struct RecordedCliHandle {
    pub role_id: String,
    pub agent_profile_id: String,
    pub cli_session_id: String,
}

/// Extracts a [`RecordedCliHandle`] from one member's response when the
/// response carries a CLI session id AND the role is bound to an agent
/// profile (the two always co-occur — only `CliAgentClient` reports one).
/// Shared by all three topology executors.
pub fn recorded_cli_handle(role: &Role, response: &ChatResponse) -> Option<RecordedCliHandle> {
    let cli_session_id = response.cli_session_id.clone()?;
    let agent_profile_id = role
        .params
        .get("agent_profile_id")
        .and_then(serde_json::Value::as_str)?
        .to_string();
    Some(RecordedCliHandle {
        role_id: role.id.clone(),
        agent_profile_id,
        cli_session_id,
    })
}

/// Everything an executor needs to drive one team run over one session.
#[derive(Debug, Clone)]
pub struct TeamRunInput {
    pub session_id: String,
    pub task: String,
    pub roles: Vec<Role>,
    pub team: Team,
    /// Fallback model id for requests (the default endpoint's resolved
    /// model, or a dialect tag / `"mixed"` when unknown).
    pub model: String,
    /// Per-binding model ids keyed by provider id / agent-profile id — the
    /// same keys as the `ProviderResolver` map. Takes precedence over
    /// [`TeamRunInput::model`] for roles that pin a binding.
    pub models: HashMap<String, String>,
    /// The workspace this run belongs to (for parallel-run isolation).
    /// `None` means the run is not bound to any workspace (legacy/CLI path).
    pub workspace_id: Option<String>,
    /// Resume handles for CLI-Agent members (ADR 0012 D8): role_id → the
    /// CLI session id persisted from that member's previous run. Loaded by
    /// team_runner before dispatch; non-CLI providers ignore the field.
    pub cli_handles: HashMap<String, String>,
}

impl TeamRunInput {
    /// Looks up a member role by id.
    pub fn role(&self, role_id: &str) -> Option<&Role> {
        self.roles.iter().find(|r| r.id == role_id)
    }

    /// Resolves a member role by id or fails with [`OrchestratorError::MemberNotFound`].
    pub fn require_role(&self, role_id: &str) -> Result<&Role, super::OrchestratorError> {
        self.role(role_id)
            .ok_or_else(|| super::OrchestratorError::MemberNotFound {
                team: self.team.name.clone(),
                member: role_id.to_string(),
            })
    }

    /// The model id one member's outbound request should carry:
    /// 1. the role's explicit `params.model_id` pin (set by the binding
    ///    panel when the user picked provider + model),
    /// 2. the resolved model of the binding the role pins
    ///    (`models[provider_id]` / `models[agent_profile_id]`),
    /// 3. the run-level fallback [`TeamRunInput::model`].
    ///
    /// CLI-bound roles resolve through 2/3 too, but the CLI client ignores
    /// `ChatRequest.model` — the value is display-only there.
    pub fn model_for(&self, role: &Role) -> String {
        if let Some(pinned) = role
            .params
            .get("model_id")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
        {
            return pinned.to_string();
        }
        role.provider_id
            .as_deref()
            .and_then(|key| self.models.get(key))
            .cloned()
            .unwrap_or_else(|| self.model.clone())
    }
}

/// `provider_id → client` map with a default fallback for roles that do not
/// pin a provider. Executors never see concrete vendor clients.
pub struct ProviderResolver {
    providers: HashMap<String, Arc<dyn LlmProvider>>,
    default: Arc<dyn LlmProvider>,
}

impl ProviderResolver {
    pub fn new(default: Arc<dyn LlmProvider>) -> Self {
        Self {
            providers: HashMap::new(),
            default,
        }
    }

    /// Builder-style registration of a named provider client.
    pub fn with(mut self, provider_id: impl Into<String>, provider: Arc<dyn LlmProvider>) -> Self {
        self.providers.insert(provider_id.into(), provider);
        self
    }

    /// The client a role should talk to (its pinned provider or the default).
    pub fn resolve(&self, role: &Role) -> Arc<dyn LlmProvider> {
        role.provider_id
            .as_ref()
            .and_then(|id| self.providers.get(id))
            .cloned()
            .unwrap_or_else(|| self.default.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::new_id;
    use crate::providers::FakeLlm;

    fn role(provider_id: Option<String>) -> Role {
        Role {
            id: new_id(),
            name: "r".into(),
            provider_id,
            provider_ids: vec![],
            system_prompt_override: None,
            tool_allowlist: vec![],
            required_capabilities: vec![],
            temperature: None,
            max_tokens: None,
            params: serde_json::json!({}),
            builtin: false,
            generated: false,
            ephemeral: false,
            source: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn resolve_prefers_pinned_provider_and_falls_back_to_default() {
        let pinned = Arc::new(FakeLlm::new("pinned", vec![]));
        let fallback = Arc::new(FakeLlm::new("default", vec![]));
        let resolver = ProviderResolver::new(fallback.clone() as Arc<dyn LlmProvider>)
            .with("p1", pinned.clone() as Arc<dyn LlmProvider>);

        assert_eq!(resolver.resolve(&role(Some("p1".into()))).id(), "pinned");
        assert_eq!(resolver.resolve(&role(None)).id(), "default");
        // Unknown provider id also falls back to the default.
        assert_eq!(
            resolver.resolve(&role(Some("missing".into()))).id(),
            "default"
        );
    }
}
