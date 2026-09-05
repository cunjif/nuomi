//! Router executor: picks the best-matching role by capability tags and runs
//! a single-role completion (SPEC AC8).

use serde_json::Value;

use crate::domain::Role;
use crate::providers::{ChatMessage, ChatRequest};

use super::input::{ProviderResolver, TeamRunInput};
use super::whiteboard::{record_turn, WhiteBoardService};
use super::OrchestratorError;

/// Outcome of a routed run.
#[derive(Debug, Clone)]
pub struct RouterOutcome {
    pub role_id: String,
    pub role_name: String,
    pub output: String,
}

/// Reads the `capabilities` string array out of `Role.params`.
pub fn capabilities_of(role: &Role) -> Vec<String> {
    role.params
        .get("capabilities")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Drives [`crate::domain::TeamTopology::Router`] teams.
pub struct RouterExecutor;

impl RouterExecutor {
    /// Deterministic selection: highest overlap with `required` wins; ties go
    /// to the earliest member. `None` when no member matches at all.
    pub fn select_role<'a>(input: &'a TeamRunInput, required: &[String]) -> Option<&'a Role> {
        let mut best: Option<(&Role, usize)> = None;
        for role_id in &input.team.member_role_ids {
            let Some(role) = input.role(role_id) else {
                continue;
            };
            let caps = capabilities_of(role);
            let score = required.iter().filter(|c| caps.contains(c)).count();
            if score > 0 && best.is_none_or(|(_, s)| score > s) {
                best = Some((role, score));
            }
        }
        best.map(|(role, _)| role)
    }

    pub async fn run(
        input: &TeamRunInput,
        providers: &ProviderResolver,
        wb: &WhiteBoardService,
        required: &[String],
    ) -> Result<RouterOutcome, OrchestratorError> {
        let role = Self::select_role(input, required)
            .ok_or_else(|| OrchestratorError::NoMatchingAgent(required.join(", ")))?;
        let provider = providers.resolve(role);

        let system = role
            .system_prompt_override
            .clone()
            .unwrap_or_else(|| format!("You are {}.", role.name));
        let request = ChatRequest {
            model: input.model.clone(),
            system_prompt: Some(system),
            messages: vec![ChatMessage::user(input.task.clone())],
            tools: Vec::new(),
            temperature: role.temperature,
            max_tokens: role.max_tokens,
            cache_retention: Default::default(),
        };

        let response = provider
            .complete(&request)
            .await
            .map_err(|e| OrchestratorError::Provider(e.to_string()))?;

        record_turn(wb, &input.session_id, role, "finding", &response.content).await?;

        Ok(RouterOutcome {
            role_id: role.id.clone(),
            role_name: role.name.clone(),
            output: response.content,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{new_id, Team, TeamTopology};
    use crate::providers::{FakeLlm, LlmProvider};
    use std::sync::Arc;

    fn role(name: &str, caps: &[&str]) -> Role {
        Role {
            id: new_id(),
            name: name.into(),
            provider_id: None,
            system_prompt_override: None,
            tool_allowlist: vec![],
            temperature: None,
            max_tokens: None,
            params: serde_json::json!({ "capabilities": caps }),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn input(roles: Vec<Role>) -> TeamRunInput {
        let ids = roles.iter().map(|r| r.id.clone()).collect();
        TeamRunInput {
            session_id: new_id(),
            task: "do things".into(),
            roles,
            team: Team {
                id: new_id(),
                name: "routed".into(),
                topology: TeamTopology::Router,
                member_role_ids: ids,
                config: serde_json::json!({}),
                created_at: 0,
                updated_at: 0,
            },
            model: "m".into(),
        }
    }

    /// Table-driven AC8: capability matching + no-match error path.
    #[tokio::test]
    async fn router_selection_table_driven() {
        struct Case {
            name: &'static str,
            required: Vec<&'static str>,
            expect_role_name: Option<&'static str>,
        }
        let coder = role("coder", &["code", "rust"]);
        let writer = role("writer", &["docs", "fast"]);
        let polyglot = role("polyglot", &["code", "docs", "fast"]);

        let cases = vec![
            Case {
                name: "exact single hit",
                required: vec!["docs"],
                expect_role_name: Some("writer"),
            },
            Case {
                name: "best overlap wins over partial",
                required: vec!["code", "docs"],
                expect_role_name: Some("polyglot"),
            },
            Case {
                name: "tie goes to earliest member",
                required: vec!["fast"],
                expect_role_name: Some("writer"),
            },
            Case {
                name: "no match",
                required: vec!["video-editing"],
                expect_role_name: None,
            },
        ];

        for case in cases {
            let input = input(vec![coder.clone(), writer.clone(), polyglot.clone()]);
            let required: Vec<String> = case.required.iter().map(|s| s.to_string()).collect();
            let selected = RouterExecutor::select_role(&input, &required);
            match (case.expect_role_name, selected) {
                (Some(expected), Some(role)) => assert_eq!(
                    role.name, expected,
                    "case '{}' picked wrong role",
                    case.name
                ),
                (None, None) => {}
                (expected, selected) => panic!(
                    "case '{}': expected {expected:?}, got {:?}",
                    case.name,
                    selected.map(|r| &r.name)
                ),
            }
        }
    }

    #[tokio::test]
    async fn run_without_match_returns_no_matching_agent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db").to_string_lossy().to_string();
        let wb = WhiteBoardService::new(path);

        let input = input(vec![role("coder", &["code"])]);
        let providers =
            ProviderResolver::new(Arc::new(FakeLlm::new("d", vec![])) as Arc<dyn LlmProvider>);
        let err = RouterExecutor::run(&input, &providers, &wb, &["sql".to_string()])
            .await
            .expect_err("must fail");
        assert!(matches!(err, OrchestratorError::NoMatchingAgent(_)));
        assert!(err.to_string().contains("sql"));
    }

    #[tokio::test]
    async fn run_routes_to_matching_role_and_records_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db").to_string_lossy().to_string();
        // Seed FK targets: session + roles.
        let db = crate::store::Db::open(&path).unwrap();
        crate::store::migrations::run(&db.0).unwrap();
        db.0.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES (?1, '', 1, 1)",
            ("s-r1",),
        )
        .unwrap();

        let mut input = input(vec![role("coder", &["code"])]);
        input.session_id = "s-r1".into();
        for r in &input.roles {
            db.0.execute(
                "INSERT INTO roles (id, name, created_at, updated_at) VALUES (?1, ?2, 1, 1)",
                (r.id.as_str(), r.name.as_str()),
            )
            .unwrap();
        }

        let providers = ProviderResolver::new(Arc::new(FakeLlm::new(
            "d",
            vec![FakeLlm::response("routed answer")],
        )) as Arc<dyn LlmProvider>);

        let wb = WhiteBoardService::new(path);
        let outcome = RouterExecutor::run(&input, &providers, &wb, &["code".to_string()])
            .await
            .expect("run");
        assert_eq!(outcome.output, "routed answer");
        assert_eq!(outcome.role_name, "coder");

        let notes = wb.read_all("s-r1").await.unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].body, "routed answer");
    }
}
