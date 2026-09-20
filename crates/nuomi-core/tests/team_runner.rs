//! M-TEAM1 integration: `run_team` end-to-end over real SQLite stores.
//!
//! Providers are materialized from seeded `provider_configs` rows pointing at
//! wiremock servers (loopback only, no real network); CLI members use the
//! local deterministic node fixture. Mirrors the store setup of
//! tests/orchestrator_pipeline.rs (FK targets: sessions + roles).

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nuomi_core::domain::{
    AgentProfile, CliFlavor, ProviderConfig, ProviderProtocol, Role, Team, TeamTopology,
};
use nuomi_core::harness::EventBus;
use nuomi_core::orchestrator::{OrchestratorError, WhiteBoardService};
use nuomi_core::providers::{MemorySecretStore, SecretStore};
use nuomi_core::services::run_team;
use nuomi_core::store::{migrations, repos};
use serde_json::{json, Value};
use tokio::time::timeout;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const SECRET_VALUE: &str = "sk-test";

/// Tempdir-backed SQLite with migrations + one session row; the guard must
/// outlive every connection run_team opens later.
struct Store {
    _dir: tempfile::TempDir,
    db_path: Arc<str>,
}

fn seed_store(session_id: &str) -> Store {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("team.db");
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

fn role_row(id: &str, name: &str, provider_id: Option<&str>, caps: &[&str]) -> Role {
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
        params: json!({ "capabilities": caps }),
        builtin: false,
        generated: false,
        ephemeral: false,
        source: None,
        created_at: 1,
        updated_at: 1,
    }
}

fn team_row(id: &str, name: &str, topology: TeamTopology, members: &[&str], config: Value) -> Team {
    Team {
        id: id.into(),
        name: name.into(),
        topology,
        member_role_ids: members.iter().map(|m| (*m).to_string()).collect(),
        config,
        created_at: 1,
        updated_at: 1,
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

type Captured = Arc<Mutex<Vec<String>>>;

/// Relay endpoint: echoes the last message content back prefixed with
/// "reviewed:", recording every received prompt for assertions.
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

fn fixture_path() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fake_cli.js")
        .to_string_lossy()
        .into_owned()
}

// ---------------------------------------------------------------- pipeline

#[tokio::test]
async fn pipeline_relays_outputs_between_members_end_to_end() {
    let session_id = "sess-pipe";
    let store = seed_store(session_id);
    let spec_server = canned_server("spec").await;
    let (review_server, review_inputs) = relay_server().await;

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-spec", "spec writer", spec_server.uri().as_str(), false),
        )
        .unwrap();
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-review", "reviewer", review_server.uri().as_str(), true),
        )
        .unwrap();
        repos::roles::insert(&conn, &role_row("r-writer", "writer", Some("p-spec"), &[])).unwrap();
        repos::roles::insert(
            &conn,
            &role_row("r-review", "reviewer", Some("p-review"), &[]),
        )
        .unwrap();
        repos::teams::insert(
            &conn,
            &team_row(
                "team-pipe",
                "pipeline crew",
                TeamTopology::Pipeline,
                &["r-writer", "r-review"],
                json!({}),
            ),
        )
        .unwrap();
    }

    let outcome = run_team(
        store.db_path.clone(),
        None,
        "team-pipe",
        session_id,
        "write the spec",
        secrets_for(&["kr-p-spec", "kr-p-review"]).await,
        None,
    )
    .await
    .expect("pipeline run");

    assert!(outcome.converged);
    assert_eq!(outcome.rounds, 2);
    assert_eq!(outcome.final_output, "reviewed: spec");

    // Stage two consumed stage one's output verbatim.
    {
        let inputs = review_inputs.lock().unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0], "spec");
    }

    // One whiteboard turn record per member, in pipeline order.
    let wb = WhiteBoardService::new(store.db_path.clone());
    let notes = wb.read_all(session_id).await.unwrap();
    let bodies: Vec<&str> = notes.iter().map(|note| note.body.as_str()).collect();
    assert_eq!(bodies, vec!["spec", "reviewed: spec"]);
}

// ------------------------------------------------------------------ router

#[tokio::test]
async fn router_hits_matching_capability_and_fails_without_match() {
    let session_id = "sess-route";
    let store = seed_store(session_id);
    let code_server = canned_server("code done").await;
    let docs_server = canned_server("docs done").await;

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-code", "coder", code_server.uri().as_str(), false),
        )
        .unwrap();
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-docs", "docs", docs_server.uri().as_str(), false),
        )
        .unwrap();
        repos::roles::insert(
            &conn,
            &role_row("r-coder", "coder", Some("p-code"), &["code"]),
        )
        .unwrap();
        repos::roles::insert(
            &conn,
            &role_row("r-docs", "writer", Some("p-docs"), &["docs"]),
        )
        .unwrap();
        repos::teams::insert(
            &conn,
            &team_row(
                "team-route",
                "router crew",
                TeamTopology::Router,
                &["r-coder", "r-docs"],
                json!({ "required": ["code"] }),
            ),
        )
        .unwrap();
    }

    let outcome = run_team(
        store.db_path.clone(),
        None,
        "team-route",
        session_id,
        "fix the bug",
        secrets_for(&["kr-p-code", "kr-p-docs"]).await,
        None,
    )
    .await
    .expect("routed run");
    assert_eq!(outcome.final_output, "code done");
    assert_eq!(outcome.rounds, 1);
    assert!(outcome.converged);

    // No member matches "sql" → clear error.
    let mut updated = team_row(
        "team-route",
        "router crew",
        TeamTopology::Router,
        &["r-coder", "r-docs"],
        json!({ "required": ["sql"] }),
    );
    updated.updated_at = 2;
    {
        let conn = connect(&store);
        repos::teams::update(&conn, &updated).unwrap();
    }
    let error = run_team(
        store.db_path.clone(),
        None,
        "team-route",
        session_id,
        "fix the bug",
        secrets_for(&["kr-p-code", "kr-p-docs"]).await,
        None,
    )
    .await
    .expect_err("no matching agent expected");
    assert!(
        matches!(error, OrchestratorError::NoMatchingAgent(ref caps) if caps.contains("sql")),
        "{error}"
    );
}

// -------------------------------------------------------------- group chat

#[tokio::test]
async fn group_chat_round_robin_respects_max_rounds_and_records_whiteboard() {
    let session_id = "sess-group";
    let store = seed_store(session_id);
    let bus = EventBus::default();
    let mut rx = bus.subscribe();
    let chat_server = canned_server("point taken").await;

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-main", "main model", chat_server.uri().as_str(), true),
        )
        .unwrap();
        for rid in ["g1", "g2", "g3"] {
            // provider_id None → resolves to the default (master) provider.
            repos::roles::insert(&conn, &role_row(rid, &format!("member-{rid}"), None, &[]))
                .unwrap();
        }
        repos::teams::insert(
            &conn,
            &team_row(
                "team-chat",
                "round table",
                TeamTopology::GroupChat,
                &["g1", "g2", "g3"],
                json!({ "max_rounds": 3, "selector": "round_robin" }),
            ),
        )
        .unwrap();
    }

    let outcome = run_team(
        store.db_path.clone(),
        Some(bus),
        "team-chat",
        session_id,
        "discuss the plan",
        secrets_for(&["kr-p-main"]).await,
        None,
    )
    .await
    .expect("group chat run");

    // Round-robin never signals convergence, so the cap ends the discussion.
    assert!(!outcome.converged);
    assert_eq!(outcome.rounds, 3);
    assert_eq!(outcome.final_output, "point taken");

    // One whiteboard record per utterance, in rotation order g1→g2→g3.
    let wb = WhiteBoardService::new(store.db_path.clone());
    let notes = wb.read_all(session_id).await.unwrap();
    assert_eq!(notes.len(), 3);
    assert_eq!(
        notes
            .iter()
            .map(|note| note.author_role_id.clone().unwrap())
            .collect::<Vec<_>>(),
        vec!["g1".to_string(), "g2".to_string(), "g3".to_string()]
    );

    // Live mirror reached the kernel bus.
    let event = timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("bus event within timeout")
        .expect("event received");
    assert_eq!(event.topic, "session.whiteboard");
}

#[tokio::test]
async fn group_chat_llm_selector_converges_before_any_turn() {
    let session_id = "sess-judge";
    let store = seed_store(session_id);
    // The judge provider answers {"index": 3}; with three members index 3 is
    // the convergence signal, so no member ever speaks.
    let judge_server = canned_server("{\"index\": 3}").await;

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-judge", "judge", judge_server.uri().as_str(), true),
        )
        .unwrap();
        for rid in ["j1", "j2", "j3"] {
            repos::roles::insert(&conn, &role_row(rid, &format!("voice-{rid}"), None, &[]))
                .unwrap();
        }
        repos::teams::insert(
            &conn,
            &team_row(
                "team-judge",
                "judged table",
                TeamTopology::GroupChat,
                &["j1", "j2", "j3"],
                json!({}),
            ),
        )
        .unwrap();
    }

    let outcome = run_team(
        store.db_path.clone(),
        None,
        "team-judge",
        session_id,
        "converge quickly",
        secrets_for(&["kr-p-judge"]).await,
        None,
    )
    .await
    .expect("group chat run");

    assert!(outcome.converged);
    assert_eq!(outcome.rounds, 0);
    assert_eq!(outcome.final_output, "");

    let wb = WhiteBoardService::new(store.db_path.clone());
    assert_eq!(wb.read_all(session_id).await.unwrap().len(), 0);
}

// ------------------------------------------------------- cli agent member

#[tokio::test]
async fn cli_agent_profile_joins_pipeline_as_team_member() {
    let session_id = "sess-cli";
    let store = seed_store(session_id);
    let spec_server = canned_server("spec ready").await;

    let profile = AgentProfile {
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
    };

    {
        let conn = connect(&store);
        repos::providers::insert_provider(
            &conn,
            &provider_config("p-spec", "spec writer", spec_server.uri().as_str(), false),
        )
        .unwrap();
        repos::agent_profiles::insert(&conn, &profile).unwrap();
        repos::roles::insert(&conn, &role_row("r-writer", "writer", Some("p-spec"), &[])).unwrap();
        // Role↔profile binding per SPEC team-shell-m1 D2b: the
        // `agent_profile_id` key in params_json wins over the pinned
        // provider_id, so stage two executes as the CLI fixture even though
        // a valid provider is pinned.
        let mut exec_role = role_row("r-exec", "executor", Some("p-spec"), &[]);
        exec_role.params = json!({ "capabilities": [], "agent_profile_id": "cli-fix" });
        repos::roles::insert(&conn, &exec_role).unwrap();
        repos::teams::insert(
            &conn,
            &team_row(
                "team-cli",
                "cli crew",
                TeamTopology::Pipeline,
                &["r-writer", "r-exec"],
                json!({}),
            ),
        )
        .unwrap();
    }

    let outcome = timeout(
        Duration::from_secs(60),
        run_team(
            store.db_path.clone(),
            None,
            "team-cli",
            session_id,
            "produce the spec",
            secrets_for(&["kr-p-spec"]).await,
            None,
        ),
    )
    .await
    .expect("run within timeout")
    .expect("pipeline run");

    assert_eq!(outcome.rounds, 2);
    // The plain-flavor fixture echoes its prompt's first line prefixed with
    // "plain says:" — proof the CLI member (not the pinned HTTP provider)
    // executed as the final stage.
    assert!(
        outcome.final_output.contains("plain says"),
        "final output: {}",
        outcome.final_output
    );

    let wb = WhiteBoardService::new(store.db_path.clone());
    let notes = wb.read_all(session_id).await.unwrap();
    assert_eq!(
        notes
            .iter()
            .map(|note| note.body.as_str())
            .collect::<Vec<_>>(),
        vec!["spec ready", outcome.final_output.as_str()]
    );
}

// ------------------------------------------------------------ error paths

#[tokio::test]
async fn missing_member_role_fails_with_member_not_found() {
    let store = seed_store("sess-missing");

    {
        let conn = connect(&store);
        repos::teams::insert(
            &conn,
            &team_row(
                "team-broken",
                "broken crew",
                TeamTopology::Pipeline,
                &["ghost-role"],
                json!({}),
            ),
        )
        .unwrap();
    }

    let error = run_team(
        store.db_path.clone(),
        None,
        "team-broken",
        "sess-missing",
        "task",
        Arc::new(MemorySecretStore::default()),
        None,
    )
    .await
    .expect_err("member lookup must fail");
    match error {
        OrchestratorError::MemberNotFound { member, .. } => assert_eq!(member, "ghost-role"),
        other => panic!("expected MemberNotFound, got {other}"),
    }
}

#[tokio::test]
async fn unknown_team_fails_with_invalid_team_not_found() {
    let store = seed_store("sess-none");

    let error = run_team(
        store.db_path.clone(),
        None,
        "no-such-team",
        "sess-none",
        "task",
        Arc::new(MemorySecretStore::default()),
        None,
    )
    .await
    .expect_err("team lookup must fail");
    match error {
        OrchestratorError::InvalidTeam(message) => assert!(message.contains("not found")),
        other => panic!("expected InvalidTeam, got {other}"),
    }
}
