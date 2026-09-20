//! M-FORM1 integration: `form_team` end-to-end over real SQLite stores.
//!
//! The planner is a wiremock loopback server (no real network) answering with
//! canned plan JSON; CLI members use the local deterministic node fixture.
//! Store setup mirrors tests/team_runner.rs.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nuomi_core::domain::{
    AgentProfile, CliFlavor, ProviderConfig, ProviderProtocol, Role, TeamTopology,
};
use nuomi_core::harness::EventBus;
use nuomi_core::orchestrator::OrchestratorError;
use nuomi_core::providers::{MemorySecretStore, SecretStore};
use nuomi_core::services::{form_team, preview_team, run_team, TeamPlan};
use nuomi_core::store::{migrations, repos};
use serde_json::{json, Value};
use tokio::time::timeout;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET_VALUE: &str = "sk-test";

/// Tempdir-backed SQLite with migrations + one session row; the guard must
/// outlive every connection form_team / run_team open later.
struct Store {
    _dir: tempfile::TempDir,
    db_path: Arc<str>,
}

fn seed_store(session_id: &str) -> Store {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("former.db");
    {
        let conn = rusqlite::Connection::open(&file).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrations::run(&conn).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES (?1, '', 1, 1)",
            [session_id],
        )
        .unwrap();
    }
    Store {
        _dir: dir,
        db_path: Arc::from(file.to_string_lossy().to_string()),
    }
}

fn connect(store: &Store) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(store.db_path.to_string()).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    conn
}

fn provider_config(id: &str, name: &str, base_url: &str, master: bool) -> ProviderConfig {
    ProviderConfig {
        id: id.into(),
        name: name.into(),
        protocol: ProviderProtocol::OpenAiCompatible,
        base_url: base_url.into(),
        keyring_ref: Some(format!("kr-{id}")),
        capabilities: vec![],
        is_master: master,
        fallback_order: None,
        params: json!({}),
        created_at: 1,
        updated_at: 1,
    }
}

fn role_row(id: &str, name: &str, provider_id: Option<&str>) -> Role {
    Role {
        id: id.into(),
        name: name.into(),
        provider_id: provider_id.map(str::to_string),
        provider_ids: provider_id.map(str::to_string).into_iter().collect(),
        system_prompt_override: Some(format!("You are {name}.")),
        tool_allowlist: vec![],
        required_capabilities: vec![],
        temperature: None,
        max_tokens: None,
        params: json!({}),
        builtin: false,
        generated: false,
        ephemeral: false,
        source: None,
        created_at: 1,
        updated_at: 1,
    }
}

fn cli_profile_fixture() -> AgentProfile {
    AgentProfile {
        id: "cli-fix".into(),
        name: "fixture executor".into(),
        adapter: "cli".into(),
        flavor: CliFlavor::Plain,
        command: "node".into(),
        args: json!([fixture_path(), "plain"]),
        env: json!({}),
        working_dir: None,
        enabled: true,
        created_at: 1,
        updated_at: 1,
        model_id: None,
        resume_args: None,
    }
}

async fn secrets_for(refs: &[&str]) -> Arc<dyn SecretStore> {
    let store = Arc::new(MemorySecretStore::default());
    for reference in refs {
        store.set(reference, SECRET_VALUE).await.unwrap();
    }
    store as Arc<dyn SecretStore>
}

/// Mock OpenAI-compatible endpoint answering every call with `content`.
async fn canned_server(content: &'static str) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": { "role": "assistant", "content": content },
                "finish_reason": "stop"
            }]
        })))
        .mount(&server)
        .await;
    server
}

fn fixture_path() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fake_cli.js")
        .to_string_lossy()
        .into_owned()
}

type Captured = Arc<Mutex<Vec<String>>>;

/// Relay endpoint echoing the last message content back prefixed with
/// "reviewed:", recording every prompt for assertions.
async fn relay_server() -> (MockServer, Captured) {
    let server = MockServer::start().await;
    let received: Captured = Arc::new(Mutex::new(Vec::new()));
    let sink = received.clone();
    Mock::given(method("POST"))
        .respond_with(move |req: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
            let prompt = body["messages"]
                .as_array()
                .and_then(|msgs| msgs.last())
                .and_then(|last| last["content"].as_str())
                .unwrap_or_default()
                .to_string();
            sink.lock().unwrap().push(prompt.clone());
            ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{
                    "message": { "role": "assistant", "content": format!("reviewed: {prompt}") },
                    "finish_reason": "stop"
                }]
            }))
        })
        .mount(&server)
        .await;
    (server, received)
}

// ------------------------------------------------------------- happy path

#[tokio::test]
async fn valid_plan_creates_roles_team_and_publishes_event() {
    let session_id = "sess-form";
    let store = seed_store(session_id);
    let bus = EventBus::default();
    let mut rx = bus.subscribe();
    let plan_server = canned_server(
        r#"{"topology":"pipeline","members":[{"kind":"role","id":"r-writer","roleName":"writer"},{"kind":"cli_profile","id":"cli-fix","roleName":"executor","systemPrompt":"execute the plan"}],"config":{"maxRounds":4},"rationale":"write then execute"}"#,
    )
    .await;

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-plan", "planner", plan_server.uri().as_str(), true),
        )
        .unwrap();
        repos::roles::insert(&conn, &role_row("r-writer", "writer", Some("p-plan"))).unwrap();
        repos::agent_profiles::insert(&conn, &cli_profile_fixture()).unwrap();
    }

    let formed = timeout(
        Duration::from_secs(30),
        form_team(
            store.db_path.clone(),
            Some(bus),
            secrets_for(&["kr-p-plan"]).await,
            None,
            Some(session_id),
            "build the feature",
            5,
        ),
    )
    .await
    .expect("formation within timeout")
    .expect("team formed");

    assert_eq!(formed.rationale, "write then execute");
    // The referenced role is reused; exactly one new role for the CLI member.
    assert_eq!(formed.created_role_ids.len(), 1);
    assert_eq!(formed.team.member_role_ids.len(), 2);
    assert_eq!(formed.team.member_role_ids[0], "r-writer");
    assert_eq!(formed.team.member_role_ids[1], formed.created_role_ids[0]);
    assert!(
        formed.team.name.starts_with("auto-"),
        "{}",
        formed.team.name
    );
    assert_eq!(formed.team.topology, TeamTopology::Pipeline);

    {
        let conn = connect(&store);
        let cli_role = repos::roles::get(&conn, &formed.created_role_ids[0]).unwrap();
        assert_eq!(cli_role.params["agent_profile_id"], "cli-fix");
        assert_eq!(cli_role.provider_id, None);
        assert_eq!(
            cli_role.system_prompt_override.as_deref(),
            Some("execute the plan")
        );
        let team = repos::teams::get(&conn, &formed.team.id).unwrap();
        assert_eq!(team.config["maxRounds"], 4);
        assert_eq!(team.config["max_rounds"], 4);
        // Referenced role row survived untouched.
        assert_eq!(repos::roles::get(&conn, "r-writer").unwrap().name, "writer");
    }

    let event = timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("bus event within timeout")
        .expect("event received");
    assert_eq!(event.topic, "team.formed");
    assert_eq!(event.payload["sessionId"], session_id);
    assert_eq!(event.payload["teamId"], formed.team.id.as_str());
    assert_eq!(event.payload["memberCount"], 2);
    assert_eq!(event.payload["rationale"], "write then execute");
}

#[tokio::test]
async fn duplicate_member_names_get_uniquified_roles() {
    let session_id = "sess-dup";
    let store = seed_store(session_id);
    // Two provider members whose planned roleName collides with an existing
    // role and with each other.
    let plan_server = canned_server(
        r#"{"topology":"group_chat","members":[{"kind":"provider","id":"p-a","roleName":"analyst"},{"kind":"provider","id":"p-b","roleName":"analyst"}],"rationale":"two views"}"#,
    )
    .await;

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-plan", "planner", plan_server.uri().as_str(), false),
        )
        .unwrap();
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-a", "alpha", "http://localhost:9/v1", false),
        )
        .unwrap();
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-b", "beta", "http://localhost:9/v1", false),
        )
        .unwrap();
        repos::roles::insert(&conn, &role_row("r-existing", "analyst", Some("p-a"))).unwrap();
    }

    let formed = form_team(
        store.db_path.clone(),
        None,
        secrets_for(&["kr-p-plan"]).await,
        None,
        Some(session_id),
        "cross check",
        5,
    )
    .await
    .expect("team formed");

    {
        let conn = connect(&store);
        let first = repos::roles::get(&conn, &formed.created_role_ids[0]).unwrap();
        let second = repos::roles::get(&conn, &formed.created_role_ids[1]).unwrap();
        // Existing role keeps its plain name; new roles get -2/-3 suffixes.
        assert_eq!(first.name, "analyst-2");
        assert_eq!(second.name, "analyst-3");
        assert_eq!(
            first.provider_id.as_deref(),
            Some("p-a"),
            "provider members pin their endpoint"
        );
        assert_eq!(second.provider_id.as_deref(), Some("p-b"));
        assert_eq!(
            repos::roles::get(&conn, "r-existing").unwrap().name,
            "analyst",
            "existing row untouched"
        );
    }
}

#[tokio::test]
async fn formed_team_runs_end_to_end_with_cli_member() {
    let session_id = "sess-e2e";
    let store = seed_store(session_id);
    let spec_server = canned_server("spec ready").await;
    let plan_server = canned_server(
        r#"{"topology":"pipeline","members":[{"kind":"role","id":"r-writer","roleName":"writer"},{"kind":"cli_profile","id":"cli-fix","roleName":"executor"}],"rationale":"draft then execute locally"}"#,
    )
    .await;

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-spec", "spec writer", spec_server.uri().as_str(), false),
        )
        .unwrap();
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-plan", "planner", plan_server.uri().as_str(), true),
        )
        .unwrap();
        repos::roles::insert(&conn, &role_row("r-writer", "writer", Some("p-spec"))).unwrap();
        repos::agent_profiles::insert(&conn, &cli_profile_fixture()).unwrap();
    }

    let formed = timeout(
        Duration::from_secs(30),
        form_team(
            store.db_path.clone(),
            None,
            secrets_for(&["kr-p-spec", "kr-p-plan"]).await,
            None,
            Some(session_id),
            "produce the spec",
            5,
        ),
    )
    .await
    .expect("formation within timeout")
    .expect("team formed");

    let outcome = timeout(
        Duration::from_secs(60),
        run_team(
            store.db_path.clone(),
            None,
            &formed.team.id,
            session_id,
            "produce the spec",
            secrets_for(&["kr-p-spec", "kr-p-plan"]).await,
            None,
        ),
    )
    .await
    .expect("run within timeout")
    .expect("pipeline run");

    assert_eq!(outcome.rounds, 2);
    // Final stage executed as the CLI fixture, proving the agent_profile_id
    // binding written by form_team resolves through run_team.
    assert!(
        outcome.final_output.contains("plain says"),
        "final output: {}",
        outcome.final_output
    );
}

#[tokio::test]
async fn planner_receives_catalog_and_task_in_prompt() {
    let session_id = "sess-prompt";
    let store = seed_store(session_id);
    let (plan_server, prompts) = relay_server().await;

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-plan", "planner", plan_server.uri().as_str(), false),
        )
        .unwrap();
        repos::roles::insert(&conn, &role_row("r-writer", "writer", Some("p-plan"))).unwrap();
        repos::agent_profiles::insert(&conn, &cli_profile_fixture()).unwrap();
    }

    // The relay echoes text back, which is not a JSON object → both attempts
    // fail, but every attempt's prompt is captured for assertions.
    let error = form_team(
        store.db_path.clone(),
        None,
        secrets_for(&["kr-p-plan"]).await,
        None,
        Some(session_id),
        "translate the docs",
        4,
    )
    .await
    .expect_err("non-json plan must fail");

    match error {
        OrchestratorError::InvalidTeam(message) => assert!(message.contains("plan invalid")),
        other => panic!("expected InvalidTeam, got {other}"),
    }

    let inputs = prompts.lock().unwrap();
    assert_eq!(inputs.len(), 2, "exactly one retry after parse failure");
    for prompt in inputs.iter() {
        assert!(prompt.contains("\"roles\""), "{prompt}");
        assert!(prompt.contains("r-writer"), "{prompt}");
        assert!(prompt.contains("cli-fix"), "{prompt}");
        assert!(prompt.contains("p-plan"), "{prompt}");
        assert!(prompt.contains("at most 4 members"), "{prompt}");
        assert!(prompt.contains("TASK:\ntranslate the docs"), "{prompt}");
    }
}

// ------------------------------------------------------------- error paths

#[tokio::test]
async fn invalid_plan_after_retry_writes_nothing() {
    let session_id = "sess-bad";
    let store = seed_store(session_id);
    let plan_server = canned_server("this is not json at all").await;

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-plan", "planner", plan_server.uri().as_str(), false),
        )
        .unwrap();
        repos::roles::insert(&conn, &role_row("r-writer", "writer", Some("p-plan"))).unwrap();
    }

    let error = form_team(
        store.db_path.clone(),
        None,
        secrets_for(&["kr-p-plan"]).await,
        None,
        Some(session_id),
        "impossible task",
        5,
    )
    .await
    .expect_err("bad plan must fail");

    match error {
        OrchestratorError::InvalidTeam(message) => assert!(message.contains("plan invalid")),
        other => panic!("expected InvalidTeam, got {other}"),
    }

    // Zero writes: baseline of one seeded role, no teams at all.
    let conn = connect(&store);
    assert_eq!(repos::roles::count(&conn).unwrap(), 1);
    assert_eq!(repos::teams::count(&conn).unwrap(), 0);
}

#[tokio::test]
async fn first_bad_plan_is_retried_then_succeeds() {
    let session_id = "sess-retry";
    let store = seed_store(session_id);
    let server = MockServer::start().await;
    // The good plan is mounted first (lower precedence); the bad response is
    // mounted last and limited to one hit, so attempt one gets junk and the
    // retry falls through to the valid plan.
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": { "role": "assistant", "content": r#"{"topology":"pipeline","members":[{"kind":"role","id":"r-writer","roleName":"writer"},{"kind":"provider","id":"p-plan","roleName":"planner"}],"rationale":"second try"}"# },
                "finish_reason": "stop"
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": { "role": "assistant", "content": "garbage {" },
                "finish_reason": "stop"
            }]
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-plan", "planner", server.uri().as_str(), false),
        )
        .unwrap();
        repos::roles::insert(&conn, &role_row("r-writer", "writer", Some("p-plan"))).unwrap();
    }

    let formed = form_team(
        store.db_path.clone(),
        None,
        secrets_for(&["kr-p-plan"]).await,
        None,
        Some(session_id),
        "retry task",
        5,
    )
    .await
    .expect("retry must recover with the valid plan");

    assert_eq!(formed.rationale, "second try");
    assert_eq!(formed.team.member_role_ids.len(), 2);
    let conn = connect(&store);
    assert_eq!(repos::roles::count(&conn).unwrap(), 2);
    assert_eq!(repos::teams::count(&conn).unwrap(), 1);
}

#[tokio::test]
async fn unknown_provider_reference_fails_before_any_write() {
    let session_id = "sess-ghost";
    let store = seed_store(session_id);
    let plan_server = canned_server(
        r#"{"topology":"router","members":[{"kind":"provider","id":"ghost-provider","roleName":"coder"},{"kind":"provider","id":"p-plan","roleName":"planner"}],"rationale":"route it"}"#,
    )
    .await;

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-plan", "planner", plan_server.uri().as_str(), false),
        )
        .unwrap();
    }

    let error = form_team(
        store.db_path.clone(),
        None,
        secrets_for(&["kr-p-plan"]).await,
        None,
        Some(session_id),
        "route work",
        5,
    )
    .await
    .expect_err("unknown member reference must fail");

    match error {
        OrchestratorError::InvalidTeam(message) => {
            assert!(message.contains("unknown members"), "{message}");
            assert!(message.contains("ghost-provider"), "{message}");
        }
        other => panic!("expected InvalidTeam, got {other}"),
    }

    let conn = connect(&store);
    assert_eq!(repos::roles::count(&conn).unwrap(), 0);
    assert_eq!(repos::teams::count(&conn).unwrap(), 0);
}

#[tokio::test]
async fn member_count_bounds_are_enforced() {
    // Below the floor: a single-member plan is rejected.
    let below_store = seed_store("sess-one");
    let single_server = canned_server(
        r#"{"topology":"pipeline","members":[{"kind":"provider","id":"p-plan","roleName":"solo"}],"rationale":"alone"}"#,
    )
    .await;
    {
        let conn = connect(&below_store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-plan", "planner", single_server.uri().as_str(), false),
        )
        .unwrap();
    }
    let below_error = form_team(
        below_store.db_path.clone(),
        None,
        secrets_for(&["kr-p-plan"]).await,
        None,
        Some("sess-one"),
        "solo task",
        5,
    )
    .await
    .expect_err("single member must be rejected");
    match below_error {
        OrchestratorError::InvalidTeam(message) => {
            assert!(message.contains("member(s)"), "{message}")
        }
        other => panic!("expected InvalidTeam, got {other}"),
    }
    {
        let conn = connect(&below_store);
        assert_eq!(repos::roles::count(&conn).unwrap(), 0);
        assert_eq!(repos::teams::count(&conn).unwrap(), 0);
    }

    // Above the ceiling: three members against max_members = 2.
    let above_store = seed_store("sess-three");
    let trio_server = canned_server(
        r#"{"topology":"group_chat","members":[{"kind":"provider","id":"p-plan","roleName":"a"},{"kind":"provider","id":"p-x","roleName":"b"},{"kind":"provider","id":"p-y","roleName":"c"}],"rationale":"crowd"}"#,
    )
    .await;
    {
        let conn = connect(&above_store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-plan", "planner", trio_server.uri().as_str(), false),
        )
        .unwrap();
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-x", "x", "http://localhost:9/v1", false),
        )
        .unwrap();
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-y", "y", "http://localhost:9/v1", false),
        )
        .unwrap();
    }
    let above_error = form_team(
        above_store.db_path.clone(),
        None,
        secrets_for(&["kr-p-plan"]).await,
        None,
        Some("sess-three"),
        "crowded task",
        2,
    )
    .await
    .expect_err("oversized plan must be rejected");
    match above_error {
        OrchestratorError::InvalidTeam(message) => {
            assert!(message.contains("member(s)"), "{message}")
        }
        other => panic!("expected InvalidTeam, got {other}"),
    }
    {
        let conn = connect(&above_store);
        assert_eq!(repos::roles::count(&conn).unwrap(), 0);
        assert_eq!(repos::teams::count(&conn).unwrap(), 0);
    }
}

#[tokio::test]
async fn empty_catalog_fails_with_no_planner_error() {
    let session_id = "sess-none";
    let store = seed_store(session_id);

    let error = form_team(
        store.db_path.clone(),
        None,
        Arc::new(MemorySecretStore::default()),
        None,
        Some(session_id),
        "any task",
        5,
    )
    .await
    .expect_err("no planner available");

    match error {
        OrchestratorError::InvalidTeam(message) => assert!(message.contains("no planner")),
        other => panic!("expected InvalidTeam, got {other}"),
    }
}

// ------------------------------------------------- preview (dry-run 打磨③a)

/// `(roles, teams, events)` row counts — the zero-write proof triple.
fn counts(store: &Store) -> (i64, i64, i64) {
    let conn = connect(store);
    let roles = repos::roles::count(&conn).unwrap();
    let teams = repos::teams::count(&conn).unwrap();
    let events: i64 = conn
        .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
        .unwrap();
    (roles, teams, events)
}

#[tokio::test]
async fn preview_team_reports_members_without_writes() {
    let session_id = "sess-preview";
    let store = seed_store(session_id);
    // The provider member's roleName collides with the existing writer role:
    // the preview must show the uniquified `writer-2` without touching it.
    let plan_server = canned_server(
        r#"{"topology":"pipeline","members":[{"kind":"role","id":"r-writer","roleName":"writer"},{"kind":"provider","id":"p-a","roleName":"writer"},{"kind":"cli_profile","id":"cli-fix","roleName":"executor"}],"config":{"maxRounds":4,"required":["code"]},"rationale":"write, review, execute"}"#,
    )
    .await;

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-plan", "planner", plan_server.uri().as_str(), true),
        )
        .unwrap();
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-a", "alpha", "http://localhost:9/v1", false),
        )
        .unwrap();
        repos::roles::insert(&conn, &role_row("r-writer", "writer", Some("p-plan"))).unwrap();
        repos::agent_profiles::insert(&conn, &cli_profile_fixture()).unwrap();
    }

    let before = counts(&store);
    let plan: TeamPlan = preview_team(
        store.db_path.clone(),
        secrets_for(&["kr-p-plan"]).await,
        None,
        "build the feature",
        5,
    )
    .await
    .expect("preview must succeed on a valid plan");

    assert_eq!(plan.topology, TeamTopology::Pipeline);
    assert_eq!(plan.rationale, "write, review, execute");
    assert_eq!(plan.max_rounds, Some(4));
    assert_eq!(plan.required, vec!["code".to_string()]);
    assert_eq!(plan.members.len(), 3);

    // Reused role: planned display name, no creation flag.
    let reused = &plan.members[0];
    assert_eq!(reused.kind, "role");
    assert_eq!(reused.ref_id, "r-writer");
    assert_eq!(reused.name, "writer");
    assert!(!reused.will_create_role);

    // Provider member: suffix computed against existing role names…
    let pinned = &plan.members[1];
    assert_eq!(pinned.kind, "provider");
    assert_eq!(pinned.ref_id, "p-a");
    assert_eq!(pinned.name, "writer-2");
    assert!(pinned.will_create_role);

    // …and CLI profile member flagged for creation too.
    let bound = &plan.members[2];
    assert_eq!(bound.kind, "cli_profile");
    assert_eq!(bound.ref_id, "cli-fix");
    assert_eq!(bound.name, "executor");
    assert!(bound.will_create_role);

    // Zero writes: roles/teams/events counts identical before and after.
    assert_eq!(counts(&store), before);
}

#[tokio::test]
async fn preview_bad_plan_fails_after_retry_with_zero_writes() {
    let session_id = "sess-preview-bad";
    let store = seed_store(session_id);
    let plan_server = canned_server("still not json {").await;

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-plan", "planner", plan_server.uri().as_str(), false),
        )
        .unwrap();
        repos::roles::insert(&conn, &role_row("r-writer", "writer", Some("p-plan"))).unwrap();
    }

    let before = counts(&store);
    let error = preview_team(
        store.db_path.clone(),
        secrets_for(&["kr-p-plan"]).await,
        None,
        "hopeless task",
        5,
    )
    .await
    .expect_err("unparseable plan must fail");

    match error {
        OrchestratorError::InvalidTeam(message) => assert!(message.contains("plan invalid")),
        other => panic!("expected InvalidTeam, got {other}"),
    }
    // Both planner attempts failed; nothing leaked into any table.
    assert_eq!(counts(&store), before);
}

#[tokio::test]
async fn form_team_persists_normally_after_preview() {
    let session_id = "sess-preview-form";
    let store = seed_store(session_id);
    let bus = EventBus::default();
    let mut rx = bus.subscribe();
    let plan_server = canned_server(
        r#"{"topology":"group_chat","members":[{"kind":"role","id":"r-writer","roleName":"writer"},{"kind":"provider","id":"p-a","roleName":"analyst"}],"config":{"maxRounds":2},"rationale":"discuss"}"#,
    )
    .await;

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-plan", "planner", plan_server.uri().as_str(), true),
        )
        .unwrap();
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-a", "alpha", "http://localhost:9/v1", false),
        )
        .unwrap();
        repos::roles::insert(&conn, &role_row("r-writer", "writer", Some("p-plan"))).unwrap();
    }

    let before = counts(&store);
    let preview = preview_team(
        store.db_path.clone(),
        secrets_for(&["kr-p-plan"]).await,
        None,
        "cross check",
        5,
    )
    .await
    .expect("preview ok");
    assert_eq!(counts(&store), before, "preview leaked writes");
    assert_eq!(preview.members[1].name, "analyst");

    // The real formation right after the preview persists normally — proof
    // the shared planning phase carries no hidden side effects.
    let formed = form_team(
        store.db_path.clone(),
        Some(bus),
        secrets_for(&["kr-p-plan"]).await,
        None,
        Some(session_id),
        "cross check",
        5,
    )
    .await
    .expect("form after preview");

    assert_eq!(formed.created_role_ids.len(), 1);
    assert_eq!(formed.team.member_role_ids.len(), 2);
    assert_eq!(
        repos::roles::get(&connect(&store), &formed.created_role_ids[0])
            .unwrap()
            .name,
        preview.members[1].name,
        "commit name matches the previewed uniquified name"
    );
    let mut after = counts(&store);
    after.0 -= 1; // the one freshly created role row
    after.1 -= 1; // the team row
    assert_eq!(after, before, "exactly one role + one team written by form");

    // Exactly one team.formed event — published by form_team only.
    let event = timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("bus event within timeout")
        .expect("event received");
    assert_eq!(event.topic, "team.formed");
}
