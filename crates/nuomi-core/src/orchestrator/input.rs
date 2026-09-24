//! Shared inputs for orchestrator executors: team run input + provider resolution.

use std::collections::HashMap;
use std::sync::Arc;

use crate::domain::{Role, Team};
use crate::providers::LlmProvider;

/// Everything an executor needs to drive one team run over one session.
#[derive(Debug, Clone)]
pub struct TeamRunInput {
    pub session_id: String,
    pub task: String,
    pub roles: Vec<Role>,
    pub team: Team,
    pub model: String,
    /// The workspace this run belongs to (for parallel-run isolation).
    /// `None` means the run is not bound to any workspace (legacy/CLI path).
    pub workspace_id: Option<String>,
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
