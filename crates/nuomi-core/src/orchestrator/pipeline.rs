//! Serial pipeline executor: roles run in team order, each consuming the
//! previous role's output (SPEC AC7).

use crate::providers::{ChatMessage, ChatRequest};

use super::input::{ProviderResolver, TeamRunInput};
use super::whiteboard::{record_turn, WhiteBoardService};
use super::OrchestratorError;

/// One completed pipeline stage.
#[derive(Debug, Clone)]
pub struct PipelineStep {
    pub role_id: String,
    pub role_name: String,
    pub output: String,
}

/// End-to-end outcome of a pipeline run.
#[derive(Debug, Clone)]
pub struct PipelineOutcome {
    pub steps: Vec<PipelineStep>,
    /// Output of the last member (the pipeline's final answer).
    pub final_output: String,
}

/// Drives [`crate::domain::TeamTopology::Pipeline`] teams.
pub struct PipelineExecutor;

impl PipelineExecutor {
    pub async fn run(
        input: &TeamRunInput,
        providers: &ProviderResolver,
        wb: &WhiteBoardService,
    ) -> Result<PipelineOutcome, OrchestratorError> {
        let mut current = input.task.clone();
        let mut steps = Vec::new();

        for role_id in &input.team.member_role_ids {
            let role = input.require_role(role_id)?;
            let provider = providers.resolve(role);

            let system = role
                .system_prompt_override
                .clone()
                .unwrap_or_else(|| format!("You are {}.", role.name));
            let request = ChatRequest {
                model: input.model.clone(),
                system_prompt: Some(system),
                messages: vec![ChatMessage::user(current.clone())],
                tools: Vec::new(),
                temperature: role.temperature,
                max_tokens: role.max_tokens,
                cache_retention: Default::default(),
            };

            let response = provider
                .complete(&request)
                .await
                .map_err(|e| OrchestratorError::Provider(e.to_string()))?;

            // Durable side effects after the model answer: whiteboard note +
            // append-only event (persist-before-anything-else discipline).
            record_turn(wb, &input.session_id, role, "finding", &response.content).await?;

            current = response.content.clone();
            steps.push(PipelineStep {
                role_id: role.id.clone(),
                role_name: role.name.clone(),
                output: response.content,
            });
        }

        Ok(PipelineOutcome {
            final_output: current,
            steps,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{new_id, Role, Team, TeamTopology};
    use std::sync::Arc;

    fn role(name: &str) -> Role {
        Role {
            id: new_id(),
            name: name.into(),
            provider_id: None,
            system_prompt_override: Some(format!("{name} prompt")),
            tool_allowlist: vec![],
            temperature: None,
            max_tokens: None,
            params: serde_json::json!({}),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[tokio::test]
    async fn missing_member_role_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db").to_string_lossy().to_string();
        let wb = WhiteBoardService::new(path);

        let team = Team {
            id: new_id(),
            name: "broken".into(),
            topology: TeamTopology::Pipeline,
            member_role_ids: vec!["ghost".into()],
            config: serde_json::json!({}),
            created_at: 0,
            updated_at: 0,
        };
        let input = TeamRunInput {
            session_id: new_id(),
            task: "task".into(),
            roles: vec![role("a")],
            team,
            model: "m".into(),
        };
        let providers =
            ProviderResolver::new(Arc::new(crate::providers::FakeLlm::new("d", vec![])));

        let err = PipelineExecutor::run(&input, &providers, &wb)
            .await
            .expect_err("must fail");
        assert!(matches!(err, OrchestratorError::MemberNotFound { .. }));
    }
}
