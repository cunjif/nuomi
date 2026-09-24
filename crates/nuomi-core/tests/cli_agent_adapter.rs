//! SPEC cli-agents-m1 C3 integration tests: CLI agent as a first-class Team
//! member (AC1), direct complete() per flavor (AC2), kill-on-drop reaping
//! (AC4). All subprocesses are the local deterministic node fixture
//! `tests/fixtures/fake_cli.js`; zero network.

use futures::StreamExt;
use nuomi_core::adapters::CliAgentClient;
use nuomi_core::domain::{new_id, AgentProfile, CliFlavor, Role, Team, TeamTopology};
use nuomi_core::orchestrator::{
    PipelineExecutor, ProviderResolver, TeamRunInput, WhiteBoardService,
};
use nuomi_core::providers::{ChatMessage, ChatRequest, FakeLlm, LlmProvider, StreamEvent};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::timeout;

fn fixture_path() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/fake_cli.js")
        .to_string_lossy()
        .into_owned()
}

fn cli_profile(flavor: CliFlavor, env: serde_json::Value) -> AgentProfile {
    AgentProfile {
        id: "cli-stage".into(),
        name: "cli specialist".into(),
        adapter: "cli".into(),
        flavor,
        command: "node".into(),
        args: json!([fixture_path(), flavor.as_str()]),
        env,
        working_dir: None,
        enabled: true,
        created_at: 0,
        updated_at: 0,
        model_id: None,
        resume_args: None,
    }
}

fn role(name: &str, provider_id: &str) -> Role {
    Role {
        id: new_id(),
        name: name.into(),
        provider_id: Some(provider_id.into()),
        provider_ids: vec![provider_id.into()],
        system_prompt_override: Some(format!("You are {name}, the specialist.")),
        tool_allowlist: vec![],
        required_capabilities: vec![],
        temperature: None,
        max_tokens: None,
        params: json!({}),
        builtin: false,
        generated: false,
        ephemeral: false,
        source: None,
        created_at: 0,
        updated_at: 0,
    }
}

/// Mirrors tests/orchestrator_pipeline.rs store setup; returns the tempdir
/// guard (must outlive every connection) and the db path.
fn setup_store(session_id: &str, roles: &[Role]) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("team.db").to_string_lossy().to_string();
    let db = nuomi_core::store::Db::open(&db_path).unwrap();
    nuomi_core::store::migrations::run(&db.0).unwrap();
    db.0.execute(
        "INSERT INTO sessions (id, title, created_at, updated_at) VALUES (?1, '', 1, 1)",
        (session_id,),
    )
    .unwrap();
    for r in roles {
        db.0.execute(
            "INSERT INTO roles (id, name, created_at, updated_at) VALUES (?1, ?2, 1, 1)",
            (r.id.as_str(), r.name.as_str()),
        )
        .unwrap();
    }
    (dir, db_path)
}

fn bare_request(user: &str) -> ChatRequest {
    ChatRequest {
        model: "fixture".into(),
        system_prompt: None,
        messages: vec![ChatMessage::user(user)],
        tools: Vec::new(),
        temperature: None,
        max_tokens: None,
        cache_retention: Default::default(),
        cache_scope: None,
        external_session_id: None,
    }
}

/// AC1: a claude_code CLI agent registered in ProviderResolver completes a
/// two-stage pipeline after a FakeLlm stage, with WhiteBoard turn records.
#[tokio::test]
async fn cli_agent_joins_pipeline_team_end_to_end() {
    let session_id = new_id();
    let r1 = role("spec-writer", "p-fake");
    let r2 = role("cli-specialist", "cli-stage");
    let roles = vec![r1.clone(), r2.clone()];
    let team = Team {
        id: new_id(),
        name: "cli crew".into(),
        topology: TeamTopology::Pipeline,
        member_role_ids: vec![r1.id.clone(), r2.id.clone()],
        config: json!({}),
        created_at: 0,
        updated_at: 0,
    };

    let (_dir, db_path) = setup_store(&session_id, &roles);

    let input = TeamRunInput {
        session_id: session_id.clone(),
        task: "produce the spec".into(),
        roles,
        team,
        model: "test-model".into(),
        workspace_id: None,
    };

    let client = CliAgentClient::new(
        cli_profile(CliFlavor::ClaudeCode, json!({})),
        vec!["node".into()],
    )
    .expect("allowlisted profile");

    let mut resolver =
        ProviderResolver::new(Arc::new(FakeLlm::new("default", vec![])) as Arc<dyn LlmProvider>);
    resolver = resolver
        .with(
            "p-fake",
            Arc::new(FakeLlm::new(
                "p-fake",
                vec![FakeLlm::response("spec ready")],
            )) as Arc<dyn LlmProvider>,
        )
        .with("cli-stage", Arc::new(client) as Arc<dyn LlmProvider>);

    let wb = WhiteBoardService::new(db_path.clone());
    let outcome = timeout(
        Duration::from_secs(60),
        PipelineExecutor::run(&input, &resolver, &wb),
    )
    .await
    .expect("pipeline within timeout")
    .expect("pipeline run");

    assert_eq!(outcome.steps.len(), 2);
    assert_eq!(outcome.steps[0].output, "spec ready");
    assert!(
        outcome.steps[1].output.contains("hello world"),
        "cli stage output: {}",
        outcome.steps[1].output
    );
    assert!(outcome.final_output.contains("hello world"));

    // One WhiteBoard turn record per member, in pipeline order.
    let notes = wb.read_all(&session_id).await.unwrap();
    assert_eq!(notes.len(), 2);
    assert_eq!(
        notes.iter().map(|n| n.body.as_str()).collect::<Vec<_>>(),
        vec!["spec ready", "hello world"]
    );
}

/// AC2: codex flavor via direct complete() — content + usage mapped.
#[tokio::test]
async fn codex_flavor_complete_returns_content_and_usage() {
    let client = CliAgentClient::new(
        cli_profile(CliFlavor::Codex, json!({})),
        vec!["node".into()],
    )
    .unwrap();
    let response = timeout(
        Duration::from_secs(30),
        client.complete(&bare_request("say hi")),
    )
    .await
    .expect("within timeout")
    .expect("complete");

    assert_eq!(response.content, "hi from codex");
    let usage = response.usage.as_ref().expect("codex reports usage");
    assert_eq!(
        (usage.prompt_tokens, usage.completion_tokens),
        (5, 4),
        "{:?}",
        response.usage
    );
    assert_eq!(response.finish_reason.as_deref(), Some("stop"));
}

/// AC2: plain flavor via direct complete() — prompt echoed through stdin.
#[tokio::test]
async fn plain_flavor_complete_receives_prompt_via_stdin() {
    let client = CliAgentClient::new(
        cli_profile(CliFlavor::Plain, json!({})),
        vec!["node".into()],
    )
    .unwrap();
    let response = timeout(
        Duration::from_secs(30),
        client.complete(&bare_request("say hi")),
    )
    .await
    .expect("within timeout")
    .expect("complete");

    assert_eq!(response.content, "plain says: User: say hi\n");
    assert!(response.usage.is_none());
    assert_eq!(response.finish_reason.as_deref(), Some("stop"));
}

/// AC4: dropping the stream mid-run reaps the child process — the fixture's
/// alive marker file stops growing after the drop.
#[tokio::test]
async fn dropping_stream_kills_child_process() {
    let dir = tempfile::tempdir().unwrap();
    let alive_path = dir.path().join("alive.txt");
    let mut profile = cli_profile(
        CliFlavor::Plain,
        json!({ "FAKE_CLI_ALIVE_FILE": alive_path.to_string_lossy() }),
    );
    // Select the fixture's `alive` mode explicitly (never exits on its own).
    profile.args = json!([fixture_path(), "alive"]);
    let client = CliAgentClient::new(profile, vec!["node".into()]).unwrap();

    let mut stream = client.stream(&bare_request("ignored"));
    let first = timeout(Duration::from_secs(10), stream.next())
        .await
        .expect("first event within timeout")
        .expect("stream yields an event")
        .expect("event is ok");
    assert!(matches!(first, StreamEvent::TextDelta(_)), "{first:?}");

    fn file_len(path: &Path) -> u64 {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    }

    let len_before_drop = file_len(&alive_path);
    assert!(len_before_drop > 0, "fixture must have started writing");

    drop(stream);

    // Poll until the marker stops growing (3 consecutive stable samples at
    // 150ms ≈ 450ms, comfortably longer than the fixture's 100ms tick).
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last = len_before_drop;
    let mut stable_samples = 0;
    while Instant::now() < deadline && stable_samples < 3 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        let current = file_len(&alive_path);
        if current == last {
            stable_samples += 1;
        } else {
            stable_samples = 0;
            last = current;
        }
    }

    assert!(
        stable_samples >= 3,
        "alive file kept growing after stream drop (child not reaped): {last} bytes"
    );
    assert_eq!(
        file_len(&alive_path),
        last,
        "file changed after stability window"
    );
}
