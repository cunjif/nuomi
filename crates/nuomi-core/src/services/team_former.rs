//! TeamFormer service (M-FORM1): LLM-planned, DB-backed team formation.
//!
//! [`form_team`] asks the default planner provider (master first, else first
//! keyed provider — same resolution as [`super::team_runner::materialize`]) to
//! compose a team from the live catalog (roles / providers / enabled CLI
//! profiles), validates the returned JSON plan **before any write**, then
//! persists new member roles plus the [`Team`] row in one blocking
//! connection. Every rejection path returns before touching the database.
//!
//! [`preview_team`] is the dry-run half of the打磨③a milestone: it runs the
//! exact same planning phase (materialize → planner call → parse/retry →
//! validation → name resolution) and returns a structured [`TeamPlan`] with
//! zero writes and zero bus events — error paths are identical to
//! [`form_team`].

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::team_runner::materialize;
use crate::domain::{new_id, now_ms, AgentProfile, ProviderConfig, Role, Team, TeamTopology};
use crate::harness::{Event, EventBus};
use crate::orchestrator::OrchestratorError;
use crate::providers::{ChatRequest, LlmProvider, SecretStore};
use crate::store::{migrations, repos, Db, StoreError};

/// Result of a successful formation: the persisted team, the ids of roles
/// freshly created for it (referenced `role` members keep their existing ids
/// and are not listed), and the planner's rationale.
#[derive(Debug, Clone)]
pub struct FormedTeam {
    pub team: Team,
    pub created_role_ids: Vec<String>,
    pub rationale: String,
}

/// One planned member of a dry-run [`TeamPlan`]: what it references, how it
/// will present itself, and whether committing would create a fresh Role row.
#[derive(Debug, Clone, Serialize)]
pub struct TeamPlanMember {
    /// `"role"` | `"provider"` | `"cli_profile"`.
    pub kind: String,
    /// Referenced role / provider / agent-profile id.
    pub ref_id: String,
    /// Display name: the planned roleName for reused roles, or the uniquified
    /// name (`base`, `base-2`, ...) a commit would give the created role.
    pub name: String,
    /// `provider` / `cli_profile` members create a new Role on commit.
    pub will_create_role: bool,
}

/// Structured result of a validated formation plan — everything the UI needs
/// for a dry-run preview without persisting anything.
#[derive(Debug, Clone, Serialize)]
pub struct TeamPlan {
    pub topology: TeamTopology,
    pub members: Vec<TeamPlanMember>,
    pub max_rounds: Option<u32>,
    pub required: Vec<String>,
    pub rationale: String,
}

/// Output of the shared planning phase: the raw parsed plan (consumed by the
/// persistence path) plus its structured preview projection.
struct PlannedFormation {
    parsed: ParsedPlan,
    preview: TeamPlan,
}

/// Shared pre-persistence phase of formation (planning + parsing + validation
/// + name resolution):
///
/// 1. materializes clients; without a default HTTP provider there is no
///    planner and the call fails with `InvalidTeam("no planner provider
///    available")`,
/// 2. reads the catalog (roles, providers, enabled agent profiles) via
///    `spawn_blocking` and embeds it in an English planning prompt,
/// 3. calls the planner once, strips optional markdown fences, parses the
///    JSON plan; a parse failure retries exactly once, then fails with
///    `InvalidTeam("plan invalid")`,
/// 4. validates member count (`2..=max_members`) and every member reference
///    against the catalog, collecting all unknowns into a single error,
///
/// then projects the plan into its structured [`TeamPlan`] preview. **Pure
/// reads** — never writes, never publishes. Error paths are identical for
/// [`form_team`] and [`preview_team`] by construction.
async fn plan_formation(
    db_path: Arc<str>,
    secrets: Arc<dyn SecretStore>,
    cwd: Option<PathBuf>,
    task: &str,
    max_members: usize,
) -> Result<PlannedFormation, OrchestratorError> {
    let materialized = materialize(db_path.clone(), secrets, cwd)
        .await
        .map_err(|error| OrchestratorError::Store(error.to_string()))?;
    let planner = materialized.default.ok_or_else(|| {
        OrchestratorError::InvalidTeam("no planner provider available".to_string())
    })?;

    let catalog = read_catalog(db_path.clone()).await?;
    let request = ChatRequest::simple(
        planner.id(),
        PLANNER_SYSTEM_PROMPT,
        &build_planner_user_message(&catalog, task, max_members),
    );

    let parsed = match parse_plan(&ask_planner(&planner, &request).await?) {
        Ok(plan) => plan,
        Err(first_error) => {
            tracing::warn!("team-formation plan rejected ({first_error}); retrying once");
            let retry = ask_planner(&planner, &request).await?;
            parse_plan(&retry)
                .map_err(|_| OrchestratorError::InvalidTeam("plan invalid".to_string()))?
        }
    };

    validate_plan(&parsed, &catalog, max_members)?;
    let (parsed, preview) = resolve_preview(parsed, &catalog);
    Ok(PlannedFormation { parsed, preview })
}

/// Dry-run preview of team formation for `task`: runs the exact same planning
/// phase as [`form_team`] (including duplicate-name suffix computation for
/// members that would create roles) and returns the structured [`TeamPlan`].
///
/// Guarantees: **zero writes** (roles/teams/events tables untouched) and
/// **zero bus events**; failures surface with the same errors as
/// [`form_team`] — no planner / invalid plan after one retry / unknown
/// member references listed in a single error.
pub async fn preview_team(
    db_path: Arc<str>,
    secrets: Arc<dyn SecretStore>,
    cwd: Option<PathBuf>,
    task: &str,
    max_members: usize,
) -> Result<TeamPlan, OrchestratorError> {
    Ok(plan_formation(db_path, secrets, cwd, task, max_members)
        .await?
        .preview)
}

/// Projects a validated [`ParsedPlan`] into its structured [`TeamPlan`]:
/// reused roles keep their planned display name, while provider/cli_profile
/// members get the uniquified name a commit would assign (suffix computed
/// here for preview only — persistence recomputes it transactionally).
/// Returns the plan unchanged alongside its projection.
fn resolve_preview(plan: ParsedPlan, catalog: &Catalog) -> (ParsedPlan, TeamPlan) {
    let mut taken: HashSet<String> = catalog.roles.iter().map(|role| role.name.clone()).collect();
    let members = plan
        .members
        .iter()
        .map(|member| match member.kind.as_str() {
            "role" => TeamPlanMember {
                kind: member.kind.clone(),
                ref_id: member.id.clone(),
                name: member.role_name.clone(),
                will_create_role: false,
            },
            // "provider" | "cli_profile" — validated above.
            kind => {
                let trimmed = member.role_name.trim();
                let base = if trimmed.is_empty() {
                    &member.id
                } else {
                    trimmed
                };
                TeamPlanMember {
                    kind: kind.to_string(),
                    ref_id: member.id.clone(),
                    name: uniquify_name(base, &mut taken),
                    will_create_role: true,
                }
            }
        })
        .collect();
    let preview = TeamPlan {
        topology: plan.topology,
        members,
        max_rounds: plan
            .config
            .max_rounds
            .filter(|rounds| *rounds > 0)
            .and_then(|rounds| u32::try_from(rounds).ok()),
        required: plan
            .config
            .required
            .clone()
            .filter(|ids| !ids.is_empty())
            .unwrap_or_default(),
        rationale: plan.rationale.clone(),
    };
    (plan, preview)
}

/// Forms and persists a team for `task`:
///
/// 1. runs the shared [`plan_formation`] phase (materialize, planner call
///    with one parse-failure retry, validation),
/// 2. persists new roles (unique names, provider pin or `agent_profile_id`
///    param binding) plus the team row on one connection,
/// 3. publishes `team.formed` on the kernel bus when one is attached.
pub async fn form_team(
    db_path: Arc<str>,
    bus: Option<EventBus>,
    secrets: Arc<dyn SecretStore>,
    cwd: Option<PathBuf>,
    session_id: Option<&str>,
    task: &str,
    max_members: usize,
) -> Result<FormedTeam, OrchestratorError> {
    let resolved = plan_formation(db_path.clone(), secrets, cwd, task, max_members).await?;

    let task = task.to_string();
    let rationale = resolved.parsed.rationale.clone();
    let (created_role_ids, team) =
        tokio::task::spawn_blocking(move || persist_formed_team(db_path, resolved.parsed, task))
            .await
            .map_err(|error| OrchestratorError::Store(error.to_string()))??;

    if let Some(bus) = bus {
        bus.publish(Event::new(
            "team.formed",
            json!({
                "sessionId": session_id,
                "teamId": team.id,
                "memberCount": team.member_role_ids.len(),
                "rationale": rationale,
            }),
        ));
    }

    Ok(FormedTeam {
        team,
        created_role_ids,
        rationale,
    })
}

// ---------------------------------------------------------------- planning

const PLANNER_SYSTEM_PROMPT: &str = "You are the team-formation planner of an agent harness. \
You compose small executable teams from the catalog of available members.";

/// Builds the English planning prompt: instructions + catalog JSON + the raw
/// task text.
fn build_planner_user_message(catalog: &Catalog, task: &str, max_members: usize) -> String {
    let catalog_json = build_catalog_json(catalog);
    format!(
        "Form a team of at least 2 and at most {max_members} members for the task below.\n\n\
         CATALOG (available members, JSON):\n{catalog_json}\n\n\
         TASK:\n{task}\n\n\
         Respond with ONLY one JSON object - no markdown fences, no commentary.\n\
         Schema:\n\
         {{\"topology\":\"pipeline\"|\"router\"|\"group_chat\",\
         \"members\":[{{\"kind\":\"role\"|\"provider\"|\"cli_profile\",\"id\":string,\
         \"roleName\":string,\"systemPrompt\":string?}}],\
         \"config\":{{\"maxRounds\":int?,\"required\":[string]?}},\"rationale\":string}}\n\
         Rules:\n\
         - kind \"role\": id must be an existing entry of catalog.roles, reused as-is.\n\
         - kind \"provider\": id must be an entry of catalog.providers; a new role pinned to it is created.\n\
         - kind \"cli_profile\": id must be an entry of catalog.cliProfiles; a new role bound to it is created.\n\
         - roleName is the member's display name inside the new team;\n\
           systemPrompt optionally overrides its system prompt."
    )
}

/// Compact JSON view of everything the planner may reference.
fn build_catalog_json(catalog: &Catalog) -> String {
    let value = json!({
        "roles": catalog
            .roles
            .iter()
            .map(|r| json!({ "id": r.id, "name": r.name }))
            .collect::<Vec<_>>(),
        "providers": catalog
            .providers
            .iter()
            .map(|p| json!({
                "id": p.id,
                "name": p.name,
                "capabilities": p.capabilities,
                "isMaster": p.is_master,
            }))
            .collect::<Vec<_>>(),
        "cliProfiles": catalog
            .profiles
            .iter()
            .filter(|p| p.enabled)
            .map(|p| json!({ "id": p.id, "name": p.name, "flavor": p.flavor.as_str() }))
            .collect::<Vec<_>>(),
    });
    serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string())
}

/// One planner round-trip returning the assistant content.
async fn ask_planner(
    planner: &Arc<dyn LlmProvider>,
    request: &ChatRequest,
) -> Result<String, OrchestratorError> {
    let response = planner
        .complete(request)
        .await
        .map_err(|error| OrchestratorError::Provider(error.to_string()))?;
    Ok(response.content)
}

// ------------------------------------------------------------ plan parsing

/// The planner's plan after fence-stripping and schema checks.
#[derive(Debug)]
struct ParsedPlan {
    topology: TeamTopology,
    members: Vec<PlannedMember>,
    config: PlanConfig,
    rationale: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlannedMember {
    kind: String,
    id: String,
    role_name: String,
    system_prompt: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanConfig {
    max_rounds: Option<i64>,
    required: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawPlan {
    #[serde(default)]
    topology: String,
    #[serde(default)]
    members: Vec<PlannedMember>,
    #[serde(default)]
    config: PlanConfig,
    #[serde(default)]
    rationale: String,
}

/// Why a planner response was rejected before validation even starts.
#[derive(Debug)]
enum PlanError {
    NotJson(String),
    MissingMembers,
    BadTopology(String),
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlanError::NotJson(error) => write!(f, "not valid JSON: {error}"),
            PlanError::MissingMembers => write!(f, "members list missing or empty"),
            PlanError::BadTopology(value) => write!(f, "unknown topology '{value}'"),
        }
    }
}

/// Parses a planner response: strips optional ``` fences, deserializes the
/// JSON object, requires a non-empty member list and a known topology.
fn parse_plan(raw: &str) -> Result<ParsedPlan, PlanError> {
    let stripped = strip_code_fences(raw);
    let plan: RawPlan =
        serde_json::from_str(stripped).map_err(|error| PlanError::NotJson(error.to_string()))?;
    if plan.members.is_empty() {
        return Err(PlanError::MissingMembers);
    }
    let normalized = plan.topology.trim().to_ascii_lowercase().replace('-', "_");
    let topology = match normalized.as_str() {
        "pipeline" => TeamTopology::Pipeline,
        "router" => TeamTopology::Router,
        "group_chat" | "groupchat" => TeamTopology::GroupChat,
        other => return Err(PlanError::BadTopology(other.to_string())),
    };
    Ok(ParsedPlan {
        topology,
        members: plan.members,
        config: plan.config,
        rationale: plan.rationale,
    })
}

/// Removes one wrapping markdown code fence (```json ... ```) if present.
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

// ---------------------------------------------------------------- validation

/// Pre-flight validation against the catalog; runs strictly before any write.
fn validate_plan(
    plan: &ParsedPlan,
    catalog: &Catalog,
    max_members: usize,
) -> Result<(), OrchestratorError> {
    if plan.members.len() < 2 || plan.members.len() > max_members {
        return Err(OrchestratorError::InvalidTeam(format!(
            "plan selects {} member(s); allowed 2..={max_members}",
            plan.members.len()
        )));
    }
    let mut unknown: Vec<String> = Vec::new();
    for member in &plan.members {
        let known = match member.kind.as_str() {
            "role" => catalog.roles.iter().any(|r| r.id == member.id),
            "provider" => catalog.providers.iter().any(|p| p.id == member.id),
            "cli_profile" => catalog
                .profiles
                .iter()
                .any(|p| p.enabled && p.id == member.id),
            _ => false,
        };
        if !known {
            unknown.push(member.id.clone());
        }
    }
    if !unknown.is_empty() {
        return Err(OrchestratorError::InvalidTeam(format!(
            "unknown members: {}",
            unknown.join(", ")
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------- persistence

/// Roles, providers and CLI profiles currently registered in the database.
struct Catalog {
    roles: Vec<Role>,
    providers: Vec<ProviderConfig>,
    profiles: Vec<AgentProfile>,
}

/// Reads the full formation catalog with one short-lived blocking connection.
async fn read_catalog(db_path: Arc<str>) -> Result<Catalog, OrchestratorError> {
    tokio::task::spawn_blocking(move || -> Result<Catalog, OrchestratorError> {
        let conn = Db::open(&db_path).map_err(store_err)?;
        migrations::run(&conn.0).map_err(store_err)?;
        Ok(Catalog {
            roles: repos::roles::list(&conn.0).map_err(store_err)?,
            providers: repos::providers::list_providers(&conn.0).map_err(store_err)?,
            profiles: repos::agent_profiles::list(&conn.0).map_err(store_err)?,
        })
    })
    .await
    .map_err(|error| OrchestratorError::Store(error.to_string()))?
}

/// Persists the validated plan atomically in ONE SQLite transaction: inserts
/// a new role per `provider`/`cli_profile` member (names uniquified against
/// the current `roles` table), reuses referenced roles verbatim, then inserts
/// the team row — any step failing rolls the whole formation back. Returns
/// `(created_role_ids, team)`.
fn persist_formed_team(
    db_path: Arc<str>,
    plan: ParsedPlan,
    task: String,
) -> Result<(Vec<String>, Team), OrchestratorError> {
    let mut conn = Db::open(&db_path).map_err(store_err)?;
    migrations::run(&conn.0).map_err(store_err)?;

    let taken: HashSet<String> = repos::roles::list(&conn.0)
        .map_err(store_err)?
        .into_iter()
        .map(|role| role.name)
        .collect();
    let mut taken = taken;

    let ts = now_ms();
    let mut member_role_ids = Vec::with_capacity(plan.members.len());
    let mut created_role_ids = Vec::new();
    let tx = conn
        .0
        .transaction()
        .map_err(|error| store_err(error.into()))?;
    for member in &plan.members {
        let role_id = match member.kind.as_str() {
            "role" => member.id.clone(),
            kind => {
                let trimmed = member.role_name.trim();
                let base = if trimmed.is_empty() {
                    &member.id
                } else {
                    trimmed
                };
                // Only plain provider members pin the endpoint; CLI-bound
                // roles resolve their client through the params convention.
                let provider_id = if kind == "provider" {
                    Some(member.id.clone())
                } else {
                    None
                };
                let role = Role {
                    id: new_id(),
                    name: uniquify_name(base, &mut taken),
                    provider_id,
                    system_prompt_override: member.system_prompt.clone(),
                    tool_allowlist: Vec::new(),
                    temperature: None,
                    max_tokens: None,
                    params: if kind == "cli_profile" {
                        json!({ "agent_profile_id": member.id })
                    } else {
                        json!({})
                    },
                    created_at: ts,
                    updated_at: ts,
                };
                repos::roles::insert(&tx, &role).map_err(store_err)?;
                created_role_ids.push(role.id.clone());
                role.id
            }
        };
        member_role_ids.push(role_id);
    }

    let team_id = new_id();
    let mut config = json!({});
    if let Some(rounds) = plan.config.max_rounds.filter(|rounds| *rounds > 0) {
        config["maxRounds"] = json!(rounds);
        // The group-chat executor consumes the snake_case key
        // (see orchestrator::group_chat::GroupChatConfig::of).
        config["max_rounds"] = json!(rounds);
    }
    if let Some(required) = plan.config.required.filter(|ids| !ids.is_empty()) {
        config["required"] = json!(required);
    }
    let name = auto_team_name(&task, &team_id);
    let team = Team {
        id: team_id,
        name,
        topology: plan.topology,
        member_role_ids,
        config,
        created_at: ts,
        updated_at: ts,
    };
    repos::teams::insert(&tx, &team).map_err(store_err)?;
    tx.commit().map_err(|error| store_err(error.into()))?;

    Ok((created_role_ids, team))
}

// ------------------------------------------------------------------ helpers

fn store_err(error: StoreError) -> OrchestratorError {
    OrchestratorError::Store(error.to_string())
}

/// Lowercases ASCII alphanumerics and folds every other character run into a
/// single `-`, trimmed at both ends. Non-ASCII input folds away entirely.
fn slug(input: &str) -> String {
    let folded: String = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let mut collapsed = String::with_capacity(folded.len());
    for c in folded.chars() {
        if c != '-' || !collapsed.ends_with('-') {
            collapsed.push(c);
        }
    }
    collapsed.trim_matches('-').to_string()
}

/// `auto-<slug of first 12 slug chars>-<first 8 chars of the team id>`; falls
/// back to `task` when nothing survives folding.
fn auto_team_name(task: &str, team_id: &str) -> String {
    let head: String = slug(task).chars().take(12).collect();
    let head = head.trim_end_matches('-');
    let head = if head.is_empty() { "task" } else { head };
    format!("auto-{head}-{}", &team_id[..8])
}

/// First free `base`, `base-2`, `base-3`, ... name; registers its result so
/// successive calls within one plan cannot collide either.
fn uniquify_name(base: &str, taken: &mut HashSet<String>) -> String {
    if taken.insert(base.to_string()) {
        return base.to_string();
    }
    let mut suffix = 2u32;
    loop {
        let candidate = format!("{base}-{suffix}");
        if taken.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEAM_ID: &str = "01890a5d-ac96-774b-bcce-b302099a8057";

    #[test]
    fn slug_folds_illegal_characters_and_trims() {
        assert_eq!(slug("Hello, World!"), "hello-world");
        assert_eq!(slug("--weird__name!!"), "weird-name");
        assert_eq!(slug("中文 fix it"), "fix-it");
        assert_eq!(slug("中文 任务"), "");
        assert_eq!(slug("!!!"), "");
        assert_eq!(slug("UPPER-case_9"), "upper-case-9");
    }

    #[test]
    fn auto_team_name_uses_first_twelve_slug_chars_and_id_prefix() {
        assert_eq!(
            auto_team_name("Refactor the storage layer", TEAM_ID),
            format!("auto-refactor-the-{}", &TEAM_ID[..8])
        );
        // A dash landing on the cut boundary is trimmed instead of doubled.
        assert_eq!(
            auto_team_name("Fix the Bug!", TEAM_ID),
            format!("auto-fix-the-bug-{}", &TEAM_ID[..8])
        );
    }

    #[test]
    fn auto_team_name_falls_back_when_slug_empty() {
        assert_eq!(
            auto_team_name("!!!", TEAM_ID),
            format!("auto-task-{}", &TEAM_ID[..8])
        );
    }

    #[test]
    fn uniquify_appends_increasing_suffixes_within_one_plan() {
        let mut taken = HashSet::new();
        assert_eq!(uniquify_name("planner", &mut taken), "planner");
        assert_eq!(uniquify_name("planner", &mut taken), "planner-2");
        assert_eq!(uniquify_name("planner", &mut taken), "planner-3");
        assert_eq!(uniquify_name("other", &mut taken), "other");
    }

    #[derive(Debug)]
    enum Expected {
        Valid {
            topology: TeamTopology,
            members: usize,
        },
        NotJson,
        MissingMembers,
        BadTopology(&'static str),
    }

    /// Table-driven parser cases (AC1): legal plans for all three
    /// topologies, fenced JSON, bad JSON, missing/empty members, missing
    /// roleName, illegal topology.
    #[test]
    fn parse_plan_table_driven() {
        let valid_body = r#"{
            "topology": "pipeline",
            "members": [
                { "kind": "role", "id": "r-writer", "roleName": "writer" },
                { "kind": "cli_profile", "id": "cli-fix", "roleName": "executor",
                  "systemPrompt": "execute the plan" }
            ],
            "config": { "maxRounds": 4, "required": ["code"] },
            "rationale": "write then execute"
        }"#;
        let cases: Vec<(&str, String, Expected)> = vec![
            (
                "plain valid json",
                valid_body.to_string(),
                Expected::Valid {
                    topology: TeamTopology::Pipeline,
                    members: 2,
                },
            ),
            (
                "wrapped in markdown fence",
                format!("```json\n{valid_body}\n```"),
                Expected::Valid {
                    topology: TeamTopology::Pipeline,
                    members: 2,
                },
            ),
            (
                "router topology legal",
                r#"{"topology":"router","members":[{"kind":"role","id":"a","roleName":"x"},{"kind":"role","id":"b","roleName":"y"}],"config":{"required":["fast"]}}"#
                    .to_string(),
                Expected::Valid {
                    topology: TeamTopology::Router,
                    members: 2,
                },
            ),
            (
                "group_chat without config key",
                r#"{"topology":"group_chat","members":[{"kind":"provider","id":"p1","roleName":"a"},{"kind":"provider","id":"p2","roleName":"b"}],"rationale":"talk"}"#
                    .to_string(),
                Expected::Valid {
                    topology: TeamTopology::GroupChat,
                    members: 2,
                },
            ),
            (
                "bad json",
                "this is not json {".to_string(),
                Expected::NotJson,
            ),
            (
                "members key missing",
                r#"{"topology":"router"}"#.to_string(),
                Expected::MissingMembers,
            ),
            (
                "empty members",
                r#"{"topology":"pipeline","members":[]}"#.to_string(),
                Expected::MissingMembers,
            ),
            (
                "member missing required roleName",
                r#"{"topology":"pipeline","members":[{"kind":"role","id":"r1"},{"kind":"role","id":"r2","roleName":"y"}]}"#
                    .to_string(),
                Expected::NotJson,
            ),
            (
                "illegal topology",
                r#"{"topology":"swarm","members":[{"kind":"role","id":"a","roleName":"x"},{"kind":"role","id":"b","roleName":"y"}]}"#
                    .to_string(),
                Expected::BadTopology("swarm"),
            ),
        ];

        for (name, raw, expected) in cases {
            let parsed = parse_plan(&raw);
            match (&expected, parsed) {
                (Expected::Valid { topology, members }, Ok(plan)) => {
                    assert_eq!(plan.topology, *topology, "{name}");
                    assert_eq!(plan.members.len(), *members, "{name}");
                    if *topology == TeamTopology::Pipeline {
                        assert_eq!(plan.rationale, "write then execute", "{name}");
                        assert_eq!(plan.config.max_rounds, Some(4), "{name}");
                        assert_eq!(
                            plan.config.required.as_deref(),
                            Some(["code".to_string()].as_slice()),
                            "{name}"
                        );
                        assert_eq!(plan.members[0].role_name, "writer", "{name}");
                        assert_eq!(
                            plan.members[1].system_prompt.as_deref(),
                            Some("execute the plan"),
                            "{name}"
                        );
                    }
                    if *topology == TeamTopology::GroupChat {
                        assert_eq!(plan.config.max_rounds, None, "{name}");
                        assert_eq!(plan.config.required, None, "{name}");
                        assert_eq!(plan.rationale, "talk", "{name}");
                    }
                    if *topology == TeamTopology::Router {
                        assert_eq!(
                            plan.config.required.as_deref(),
                            Some(["fast".to_string()].as_slice()),
                            "{name}"
                        );
                    }
                }
                (Expected::NotJson, Err(PlanError::NotJson(_))) => {}
                (Expected::MissingMembers, Err(PlanError::MissingMembers)) => {}
                (Expected::BadTopology(value), Err(PlanError::BadTopology(got))) => {
                    assert_eq!(got, *value, "{name}")
                }
                (expected, parsed) => panic!("{name}: expected {expected:?}, got {parsed:?}"),
            }
        }
    }

    #[test]
    fn strip_code_fences_handles_missing_and_partial_fences() {
        assert_eq!(strip_code_fences("  plain "), "plain");
        assert_eq!(strip_code_fences("```\n{}\n```"), "{}");
        assert_eq!(strip_code_fences("```json\r\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fences("{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_code_fences("no newline fence```"), "no newline fence");
    }
}
