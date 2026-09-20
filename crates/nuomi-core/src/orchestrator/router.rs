//! Router executor: picks the best-matching role by capability tags and runs
//! a single-role completion (SPEC AC8). When no team member matches, an
//! optional [`CapabilityFallback`] (the services-layer capability router)
//! may resolve — and optionally create an ephemeral temp role for — the run.

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

/// A role handed back by a [`CapabilityFallback`] when no team member
/// matches. `temp = true` marks an ephemeral role the executor must clean
/// up (delete) after the run.
pub struct FallbackSelection {
    pub role: Role,
    pub temp: bool,
}

/// Escape hatch from "no team member matches" onto the capability-router
/// system (services/capability_router). Implementations resolve a role for
/// the required capability tags from the live catalog, creating an
/// ephemeral `temp-*` role when only an unbound provider can serve.
pub trait CapabilityFallback: Send + Sync {
    /// `Ok(None)` = nothing in the catalog can serve `required`.
    /// Short-lived blocking SQLite access (single-digit ms) — acceptable
    /// inside the executor's per-run path.
    fn resolve(&self, required: &[String]) -> Result<Option<FallbackSelection>, String>;

    /// Removes a previously created temp role (GC hook). No-op default for
    /// stateless fallbacks.
    fn cleanup(&self, role_id: &str) {
        let _ = role_id;
    }
}

/// Drives [`crate::domain::TeamTopology::Router`] teams.
pub struct RouterExecutor;

impl RouterExecutor {
    /// Deterministic selection: highest overlap with `required` wins; ties go
    /// to the earliest member. `None` when no member matches at all.
    /// Matching consults both the legacy `params.capabilities` tags and the
    /// typed `required_capabilities` column (migration 0010).
    pub fn select_role<'a>(input: &'a TeamRunInput, required: &[String]) -> Option<&'a Role> {
        let mut best: Option<(&Role, usize)> = None;
        for role_id in &input.team.member_role_ids {
            let Some(role) = input.role(role_id) else {
                continue;
            };
            let caps = role.capability_tags();
            let score = required.iter().filter(|c| caps.contains(c)).count();
            if score > 0 && best.is_none_or(|(_, s)| score > s) {
                best = Some((role, score));
            }
        }
        best.map(|(role, _)| role)
    }

    /// Back-compat entry point without a capability fallback.
    pub async fn run(
        input: &TeamRunInput,
        providers: &ProviderResolver,
        wb: &WhiteBoardService,
        required: &[String],
    ) -> Result<RouterOutcome, OrchestratorError> {
        Self::run_with_fallback(input, providers, wb, required, None).await
    }

    /// Router run with an optional capability-router fallback: when no team
    /// member matches, the fallback may resolve a role (possibly an
    /// ephemeral temp role, which is cleaned up after a successful run).
    pub async fn run_with_fallback(
        input: &TeamRunInput,
        providers: &ProviderResolver,
        wb: &WhiteBoardService,
        required: &[String],
        fallback: Option<&dyn CapabilityFallback>,
    ) -> Result<RouterOutcome, OrchestratorError> {
        let mut temp_role_id: Option<String> = None;
        let role = match Self::select_role(input, required) {
            Some(role) => role.clone(),
            None => {
                let selection = match fallback {
                    Some(f) => f.resolve(required).map_err(OrchestratorError::Provider)?,
                    None => None,
                };
                match selection {
                    Some(FallbackSelection { role, temp }) => {
                        if temp {
                            temp_role_id = Some(role.id.clone());
                        }
                        role
                    }
                    None => return Err(OrchestratorError::NoMatchingAgent(required.join(", "))),
                }
            }
        };

        let provider = providers.resolve(&role);

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
            cache_scope: None,
            external_session_id: None,
        };

        let response = provider
            .complete(&request)
            .await
            .map_err(|e| OrchestratorError::Provider(e.to_string()))?;

        record_turn(wb, &input.session_id, &role, "finding", &response.content).await?;

        // GC hook: a successfully used temp role is removed immediately;
        // failed attempts are left for the run-end ephemeral GC
        // (services::capability_router::cleanup_expired_temps).
        if let Some(temp_id) = temp_role_id {
            if let Some(f) = fallback {
                f.cleanup(&temp_id);
            }
        }

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
            provider_ids: vec![],
            system_prompt_override: None,
            tool_allowlist: vec![],
            required_capabilities: caps
                .iter()
                .filter_map(|c| crate::domain::Capability::parse(c))
                .collect(),
            temperature: None,
            max_tokens: None,
            params: serde_json::json!({ "capabilities": caps }),
            builtin: false,
            generated: false,
            ephemeral: false,
            source: None,
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

    /// Fallback path: no member matches → the fallback hands over a temp
    /// role → the run succeeds and the temp role is cleaned up.
    #[tokio::test]
    async fn fallback_resolves_temp_role_and_cleans_up() {
        struct FakeFallback(String);
        impl CapabilityFallback for FakeFallback {
            fn resolve(&self, required: &[String]) -> Result<Option<FallbackSelection>, String> {
                let caps: Vec<crate::domain::Capability> = required
                    .iter()
                    .filter_map(|c| crate::domain::Capability::parse(c))
                    .collect();
                let mut role = role("temp-reasoning-ab12", &[]);
                role.required_capabilities = caps;
                role.ephemeral = true;
                // Mirror the real router: persist the temp role so the
                // whiteboard FK on author_role_id is satisfiable.
                let db = crate::store::Db::open(&self.0).map_err(|e| e.to_string())?;
                crate::store::repos::roles::insert(&db.0, &role).map_err(|e| e.to_string())?;
                Ok(Some(FallbackSelection { role, temp: true }))
            }
            fn cleanup(&self, role_id: &str) {
                CLEANED.with(|c| c.borrow_mut().push(role_id.to_string()));
            }
        }
        thread_local! {
            static CLEANED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db").to_string_lossy().to_string();
        let db = crate::store::Db::open(&path).unwrap();
        crate::store::migrations::run(&db.0).unwrap();
        db.0.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES (?1, '', 1, 1)",
            ("s-r2",),
        )
        .unwrap();

        let input = input(vec![role("coder", &["code"])]);
        let mut input = input;
        input.session_id = "s-r2".into();

        let providers = ProviderResolver::new(Arc::new(FakeLlm::new(
            "d",
            vec![FakeLlm::response("fb")],
        )) as Arc<dyn LlmProvider>);
        let wb = WhiteBoardService::new(path.clone());

        let outcome = RouterExecutor::run_with_fallback(
            &input,
            &providers,
            &wb,
            &["reasoning".to_string()],
            Some(&FakeFallback(path)),
        )
        .await
        .expect("fallback run");
        assert_eq!(outcome.output, "fb");
        assert_eq!(outcome.role_name, "temp-reasoning-ab12");
        // The temp role (its uuid id, not the display name) was GC'd.
        let cleaned_id = outcome.role_id.clone();
        CLEANED.with(|c| assert_eq!(c.borrow().as_slice(), [cleaned_id.as_str()]));
    }

    /// Fallback returning `None` degrades to the original NoMatchingAgent.
    #[tokio::test]
    async fn fallback_none_still_errors_with_no_matching_agent() {
        struct NoFallback;
        impl CapabilityFallback for NoFallback {
            fn resolve(&self, _required: &[String]) -> Result<Option<FallbackSelection>, String> {
                Ok(None)
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db").to_string_lossy().to_string();
        let wb = WhiteBoardService::new(path);
        let input = input(vec![role("coder", &["code"])]);
        let providers =
            ProviderResolver::new(Arc::new(FakeLlm::new("d", vec![])) as Arc<dyn LlmProvider>);
        let err = RouterExecutor::run_with_fallback(
            &input,
            &providers,
            &wb,
            &["reasoning".to_string()],
            Some(&NoFallback),
        )
        .await
        .expect_err("must fail");
        assert!(matches!(err, OrchestratorError::NoMatchingAgent(_)));
    }
}
