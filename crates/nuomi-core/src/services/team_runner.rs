//! TeamRunner service (M-TEAM1 Wave2): the DB-backed execution surface for
//! Teams.
//!
//! [`materialize`] turns persisted configuration into live clients —
//! `provider_configs` rows become OpenAI-/Anthropic-compatible HTTP clients
//! (API keys fetched from the given [`SecretStore`] via `keyring_ref`) and
//! enabled `agent_profiles` rows become [`CliAgentClient`]s. [`run_team`]
//! then loads a team plus its member roles from SQLite, wires a
//! [`ProviderResolver`] and dispatches to the topology's orchestrator
//! executor (pipeline / router / group chat). All SQLite access runs inside
//! `spawn_blocking`, mirroring `orchestrator::whiteboard`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use crate::adapters::CliAgentClient;
use crate::domain::{AgentProfile, ProviderConfig, ProviderProtocol, Role, Team, TeamTopology};
use crate::harness::EventBus;
use crate::orchestrator::{
    GroupChatExecutor, LlmSelector, OrchestratorError, PipelineExecutor, ProviderResolver,
    RoundRobinSelector, RouterExecutor, SpeakerSelector, TeamRunInput, WhiteBoardService,
};
use crate::providers::{
    AnthropicCompatibleClient, LlmProvider, OpenAiCompatibleClient, SecretStore,
};
use crate::store::{migrations, repos, Db, StoreError};
use crate::{CoreError, CoreResult};

/// Every client materialized from the database for one run.
pub struct MaterializedProviders {
    /// Keyed by provider id (LLM endpoints) or agent-profile id (CLI agents);
    /// `Role.provider_id` may point at either kind of key.
    pub providers: HashMap<String, Arc<dyn LlmProvider>>,
    /// Master provider if any config is flagged master, else the first
    /// provider in list order; `None` when no provider config exists at all
    /// (the caller decides what that means).
    pub default: Option<Arc<dyn LlmProvider>>,
    /// Non-fatal skip reasons collected while materializing.
    pub warnings: Vec<String>,
}

/// End state of one team run.
#[derive(Debug, Clone)]
pub struct TeamRunOutcome {
    /// Pipeline: last member's output; router: the routed member's output;
    /// group chat: the last utterance (empty when none happened).
    pub final_output: String,
    /// True when the run finished deterministically (pipeline/router) or the
    /// group chat ended via its convergence signal rather than the round cap.
    pub converged: bool,
    /// Executed turns: pipeline stages / 1 for router / discussion rounds.
    pub rounds: usize,
}

/// Builds every usable client from `provider_configs` + enabled
/// `agent_profiles`.
///
/// - Provider configs become protocol-specific HTTP clients; the API key is
///   fetched from `secrets` via `keyring_ref`. A missing reference or an
///   unreachable secret skips the entry with a collected warning instead of
///   failing the whole materialization.
/// - Enabled CLI profiles become [`CliAgentClient`]s whose allowlist is their
///   own command basename (explicit user configuration = self authorization).
///   `cwd` seeds each client's default working directory.
pub async fn materialize(
    db_path: Arc<str>,
    secrets: Arc<dyn SecretStore>,
    cwd: Option<PathBuf>,
) -> Result<MaterializedProviders, CoreError> {
    let (configs, profiles) = read_catalog(db_path).await?;

    let mut providers: HashMap<String, Arc<dyn LlmProvider>> = HashMap::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut default: Option<Arc<dyn LlmProvider>> = None;

    for config in &configs {
        let Some(api_key) = resolve_api_key(&secrets, config).await else {
            warnings.push(format!(
                "provider '{}' skipped: no api key via keyring_ref {:?} or env {}",
                config.name,
                config.keyring_ref,
                env_key_name(&config.id)
            ));
            continue;
        };
        // Per-provider proxy (settings.proxy): a dedicated pool routed through
        // it. An unparseable proxy skips the provider — silently falling back
        // to a direct connection would leak traffic the user wanted proxied.
        let proxy = crate::domain::entities::ProviderSettings::from_params(&config.params).proxy;
        let http = match crate::providers::pool::client_for_endpoint(proxy.as_deref()) {
            Ok(http) => http,
            Err(e) => {
                warnings.push(format!("provider '{}' skipped: {e}", config.name));
                continue;
            }
        };
        let client: Arc<dyn LlmProvider> = match config.protocol {
            ProviderProtocol::OpenAiCompatible => Arc::new(
                OpenAiCompatibleClient::new(config.base_url.clone(), api_key)
                    .with_http_client(http),
            ),
            ProviderProtocol::AnthropicCompatible => Arc::new(
                AnthropicCompatibleClient::new(config.base_url.clone(), api_key)
                    .with_http_client(http),
            ),
        };
        // Master wins over list order; otherwise the first entry sticks.
        if config.is_master || default.is_none() {
            default = Some(client.clone());
        }
        providers.insert(config.id.clone(), client);
    }

    for profile in profiles.into_iter().filter(|p| p.enabled) {
        let allowlist = vec![command_base_name(&profile.command)];
        let mut client = match CliAgentClient::new(profile.clone(), allowlist) {
            Ok(client) => client,
            Err(error) => {
                warnings.push(format!("agent profile '{}' skipped: {error}", profile.name));
                continue;
            }
        };
        if let Some(dir) = &cwd {
            client = client.with_default_cwd(dir.clone());
        }
        providers.insert(profile.id.clone(), Arc::new(client));
    }

    Ok(MaterializedProviders {
        providers,
        default,
        warnings,
    })
}

/// Runs one team end-to-end over the shared database:
///
/// 1. loads the team and its member roles (`member_role_ids` are resolved in
///    order; a missing role fails with [`OrchestratorError::MemberNotFound`]),
/// 2. materializes all clients,
/// 3. dispatches by topology onto pipeline / router / group-chat executors,
///    recording every turn on the session whiteboard (and mirroring to `bus`
///    when given).
pub async fn run_team(
    db_path: Arc<str>,
    bus: Option<EventBus>,
    team_id: &str,
    session_id: &str,
    task: &str,
    secrets: Arc<dyn SecretStore>,
    cwd: Option<PathBuf>,
) -> Result<TeamRunOutcome, OrchestratorError> {
    let (team, mut roles) = load_team(db_path.clone(), team_id).await?;
    let materialized = materialize(db_path.clone(), secrets, cwd)
        .await
        .map_err(|error| OrchestratorError::Store(error.to_string()))?;

    // SPEC team-shell-m1 D2b: a Role binds to an agent profile through the
    // `params_json.agent_profile_id` convention key, which takes priority
    // over `provider_id`. The schema is untouched — the override happens on
    // the in-memory copies; CLI clients are registered under their
    // profile id, so plain resolver lookup does the rest.
    for role in &mut roles {
        if let Some(profile_id) = role.params.get("agent_profile_id").and_then(Value::as_str) {
            role.provider_id = Some(profile_id.to_string());
        }
    }

    let has_default = materialized.default.is_some();
    let (default_provider, model) = match materialized.default.clone() {
        Some(provider) => {
            let id = provider.id().to_string();
            (provider, id)
        }
        None => {
            // No single default endpoint: fall back to any materialized
            // client so roles that pin their own provider still resolve.
            let fallback = materialized
                .providers
                .values()
                .next()
                .cloned()
                .ok_or_else(|| {
                    OrchestratorError::InvalidTeam(
                        "no provider config or enabled agent profile".to_string(),
                    )
                })?;
            (fallback, "mixed".to_string())
        }
    };

    let mut resolver = ProviderResolver::new(default_provider.clone());
    for (id, provider) in &materialized.providers {
        resolver = resolver.with(id.clone(), provider.clone());
    }

    let mut wb = WhiteBoardService::new(db_path.clone());
    if let Some(bus) = bus {
        wb = wb.with_bus(bus);
    }

    let input = TeamRunInput {
        session_id: session_id.to_string(),
        task: task.to_string(),
        roles,
        team: team.clone(),
        model,
    };

    match team.topology {
        TeamTopology::Pipeline => {
            let outcome = PipelineExecutor::run(&input, &resolver, &wb).await?;
            Ok(TeamRunOutcome {
                final_output: outcome.final_output,
                converged: true,
                rounds: outcome.steps.len(),
            })
        }
        TeamTopology::Router => {
            let required: Vec<String> = team
                .config
                .get("required")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            // Capability-router fallback: when no member role matches the
            // required capabilities, the DB-backed router may resolve (or
            // ephemeral-create + auto-GC) a role from the provider catalog.
            let fallback = super::capability_router::DbCapabilityFallback::new(db_path.clone());
            let outcome = RouterExecutor::run_with_fallback(
                &input,
                &resolver,
                &wb,
                &required,
                Some(&fallback),
            )
            .await?;
            Ok(TeamRunOutcome {
                final_output: outcome.output,
                converged: true,
                rounds: 1,
            })
        }
        TeamTopology::GroupChat => {
            let selector: Arc<dyn SpeakerSelector> =
                if !has_default || wants_round_robin(team.config.get("selector")) {
                    Arc::new(RoundRobinSelector)
                } else {
                    Arc::new(LlmSelector::new(default_provider, input.model.clone()))
                };
            let outcome = GroupChatExecutor::new(selector)
                .run(&input, &resolver, &wb)
                .await?;
            Ok(TeamRunOutcome {
                final_output: outcome
                    .transcript
                    .last()
                    .map(|turn| turn.text.clone())
                    .unwrap_or_default(),
                converged: outcome.converged,
                rounds: outcome.rounds,
            })
        }
    }
}

// ---------------------------------------------------------------- helpers

/// Reads provider configs + agent profiles with one short-lived connection.
async fn read_catalog(db_path: Arc<str>) -> CoreResult<(Vec<ProviderConfig>, Vec<AgentProfile>)> {
    tokio::task::spawn_blocking(
        move || -> CoreResult<(Vec<ProviderConfig>, Vec<AgentProfile>)> {
            let conn = Db::open(&db_path)?;
            migrations::run(&conn.0)?;
            let configs = repos::providers::list_providers(&conn.0)?;
            let profiles = repos::agent_profiles::list(&conn.0)?;
            Ok((configs, profiles))
        },
    )
    .await
    .map_err(join_err)?
}

/// Resolves the API key for one provider config: keyring reference first,
/// then the documented environment-variable fallback. `None` means "skip
/// this provider with a warning" — never a hard failure.
async fn resolve_api_key(
    secrets: &Arc<dyn SecretStore>,
    config: &ProviderConfig,
) -> Option<String> {
    if let Some(reference) = config.keyring_ref.as_deref() {
        if let Ok(key) = secrets.get(reference).await {
            return Some(key);
        }
    }
    let name = env_key_name(&config.id);
    std::env::var(&name).ok().filter(|value| !value.is_empty())
}

/// Environment-variable fallback name for provider keys (naming convention
/// frozen here per SPEC team-shell-m1 D2a/§5): `NUOMI_PROVIDER_<ID>_API_KEY`
/// with `<ID>` uppercased and non-alphanumeric runs mapped to `_`.
fn env_key_name(provider_id: &str) -> String {
    let sanitized: String = provider_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect();
    format!("NUOMI_PROVIDER_{sanitized}_API_KEY")
}

/// Loads one team plus its member roles; missing rows map onto orchestrator
/// errors (`InvalidTeam("not found")` / `MemberNotFound`).
async fn load_team(
    db_path: Arc<str>,
    team_id: &str,
) -> Result<(Team, Vec<Role>), OrchestratorError> {
    let team_id = team_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<(Team, Vec<Role>), OrchestratorError> {
        let conn = Db::open(&db_path).map_err(to_store_error)?;
        migrations::run(&conn.0).map_err(to_store_error)?;
        let team = match repos::teams::get(&conn.0, &team_id) {
            Ok(team) => team,
            Err(StoreError::NotFound { .. }) => {
                return Err(OrchestratorError::InvalidTeam("not found".to_string()));
            }
            Err(error) => return Err(OrchestratorError::Store(error.to_string())),
        };
        let roles = team
            .member_role_ids
            .iter()
            .map(|role_id| load_member(&conn.0, &team, role_id))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((team, roles))
    })
    .await
    .map_err(|error| OrchestratorError::Store(error.to_string()))?
}

fn load_member(
    conn: &rusqlite::Connection,
    team: &Team,
    role_id: &str,
) -> Result<Role, OrchestratorError> {
    repos::roles::get(conn, role_id).map_err(|error| match error {
        StoreError::NotFound { .. } => OrchestratorError::MemberNotFound {
            team: team.name.clone(),
            member: role_id.to_string(),
        },
        other => OrchestratorError::Store(other.to_string()),
    })
}

/// True when the team's selector config explicitly asks for round-robin
/// (`"round_robin"` string, or an object carrying `"kind"`/`"type"`).
fn wants_round_robin(selector_config: Option<&Value>) -> bool {
    match selector_config {
        Some(Value::String(kind)) => kind == "round_robin",
        Some(Value::Object(map)) => {
            map.get("kind")
                .or_else(|| map.get("type"))
                .and_then(Value::as_str)
                == Some("round_robin")
        }
        _ => false,
    }
}

/// Basename of an executable path, handling both Windows `\` and Unix `/`
/// separators (comparison itself happens case-insensitively inside
/// [`CliAgentClient`]).
fn command_base_name(command: &str) -> String {
    command
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(command)
        .trim()
        .to_string()
}

fn to_store_error(error: StoreError) -> OrchestratorError {
    OrchestratorError::Store(error.to_string())
}

fn join_err(error: tokio::task::JoinError) -> CoreError {
    CoreError::Store(StoreError::Sqlite(rusqlite::Error::ToSqlConversionFailure(
        Box::new(error),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{now_ms, CliFlavor};
    use crate::providers::MemorySecretStore;
    use serde_json::json;

    fn cfg(
        id: &str,
        name: &str,
        keyring_ref: Option<&str>,
        master: bool,
        at: i64,
    ) -> ProviderConfig {
        ProviderConfig {
            id: id.into(),
            name: name.into(),
            protocol: ProviderProtocol::OpenAiCompatible,
            base_url: "http://localhost:9/v1".into(),
            keyring_ref: keyring_ref.map(str::to_string),
            capabilities: vec![],
            is_master: master,
            fallback_order: None,
            params: json!({}),
            created_at: at,
            updated_at: at,
        }
    }

    /// Creates the database file with migrations applied; the returned guard
    /// must outlive every later connection.
    fn temp_db() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.db");
        let conn = rusqlite::Connection::open(&file).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrations::run(&conn).unwrap();
        (dir, file)
    }

    #[tokio::test]
    async fn materializes_keyed_providers_and_skips_unreachable_keys() {
        let (_dir, file) = temp_db();
        {
            let conn = rusqlite::Connection::open(&file).unwrap();
            repos::providers::insert_provider(
                &conn,
                &cfg("p1", "primary", Some("kr/p1"), false, 1),
            )
            .unwrap();
            repos::providers::insert_provider(&conn, &cfg("p2", "keyless", None, false, 2))
                .unwrap();
        }
        let secrets = Arc::new(MemorySecretStore::default());
        secrets.set("kr/p1", "sk-test").await.unwrap();

        let result = materialize(Arc::from(file.to_string_lossy().to_string()), secrets, None)
            .await
            .unwrap();

        assert!(result.providers.contains_key("p1"));
        assert!(!result.providers.contains_key("p2"));
        assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
        assert!(
            result.warnings[0].contains("keyless"),
            "{:?}",
            result.warnings
        );
        let default = result
            .default
            .expect("first keyed provider becomes default");
        assert_eq!(default.id(), "openai_compatible");
    }

    #[tokio::test]
    async fn master_flag_beats_list_order_for_default() {
        let (_dir, file) = temp_db();
        {
            let conn = rusqlite::Connection::open(&file).unwrap();
            repos::providers::insert_provider(&conn, &cfg("early", "e", Some("k/e"), false, 1))
                .unwrap();
            let mut late_master = cfg("late", "l", Some("k/l"), true, 2);
            late_master.protocol = ProviderProtocol::AnthropicCompatible;
            repos::providers::insert_provider(&conn, &late_master).unwrap();
        }
        let secrets = Arc::new(MemorySecretStore::default());
        secrets.set("k/e", "a").await.unwrap();
        secrets.set("k/l", "b").await.unwrap();

        let result = materialize(Arc::from(file.to_string_lossy().to_string()), secrets, None)
            .await
            .unwrap();
        assert_eq!(result.default.expect("master").id(), "anthropic_compatible");
    }

    #[tokio::test]
    async fn registers_enabled_cli_profiles_under_their_profile_id() {
        let (_dir, file) = temp_db();
        let profile = AgentProfile {
            id: "cli-fix-1".into(),
            name: "fixture agent".into(),
            adapter: "cli".into(),
            flavor: CliFlavor::Plain,
            command: r"C:\Tools\Node\node.exe".into(),
            args: json!(["ignored"]),
            env: json!({}),
            working_dir: None,
            enabled: true,
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let disabled = AgentProfile {
            id: "cli-off".into(),
            name: "disabled agent".into(),
            enabled: false,
            ..profile.clone()
        };
        {
            let conn = rusqlite::Connection::open(&file).unwrap();
            repos::agent_profiles::insert(&conn, &profile).unwrap();
            repos::agent_profiles::insert(&conn, &disabled).unwrap();
        }

        let result = materialize(
            Arc::from(file.to_string_lossy().to_string()),
            Arc::new(MemorySecretStore::default()),
            None,
        )
        .await
        .unwrap();

        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert!(!result.providers.contains_key("cli-off"));
        let client = result
            .providers
            .get("cli-fix-1")
            .expect("enabled profile registered under its profile id");
        assert_eq!(client.id(), "cli-fix-1");
    }

    #[tokio::test]
    async fn empty_catalog_yields_no_providers_and_no_default() {
        let (_dir, file) = temp_db();
        let result = materialize(
            Arc::from(file.to_string_lossy().to_string()),
            Arc::new(MemorySecretStore::default()),
            None,
        )
        .await
        .unwrap();
        assert!(result.providers.is_empty());
        assert!(result.default.is_none());
        assert!(result.warnings.is_empty());
    }

    #[tokio::test]
    async fn environment_variable_fallback_supplies_missing_key() {
        // Unique provider id ⇒ unique env name; no interference between
        // parallel tests. "env.p1" sanitizes to ENV_P1.
        std::env::set_var("NUOMI_PROVIDER_ENV_P1_API_KEY", "sk-from-env");
        let (_dir, file) = temp_db();
        {
            let conn = rusqlite::Connection::open(&file).unwrap();
            repos::providers::insert_provider(&conn, &cfg("env.p1", "env keyed", None, false, 1))
                .unwrap();
        }
        let result = materialize(
            Arc::from(file.to_string_lossy().to_string()),
            Arc::new(MemorySecretStore::default()),
            None,
        )
        .await
        .unwrap();
        assert!(
            result.providers.contains_key("env.p1"),
            "{:?}",
            result.warnings
        );
        assert_eq!(
            result.default.expect("env fallback becomes default").id(),
            "openai_compatible"
        );
    }

    /// Guards the basename helper against path-separator drift.
    #[test]
    fn command_base_name_handles_windows_and_unix_separators() {
        assert_eq!(command_base_name(r"C:\Tools\Node.EXE"), "Node.EXE");
        assert_eq!(command_base_name("/usr/local/bin/node"), "node");
        assert_eq!(command_base_name("python"), "python");
    }
}
