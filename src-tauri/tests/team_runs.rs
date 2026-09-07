//! M-TEAM1 T4 integration tests (SPEC docs/specs/team-shell-m1.md):
//! roles/teams CRUD over the IPC impl layer, `run_team_on_task` Run
//! lifecycle (state_changed iron rule), `run_team_session`, and whiteboard
//! listing. CLI members execute the deterministic node fixture
//! `../crates/nuomi-core/tests/fixtures/fake_cli.js`; zero network.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use nuomi_core::domain::{ProviderConfig, ProviderProtocol, Role};
use nuomi_core::facade::ProviderSource;
use nuomi_core::providers::{ChatResponse, MemorySecretStore};
use nuomi_core::store::{migrations, repos, Db};
use nuomi_shell_lib::commands::{self, RoleInput, TeamInput, TeamTopologyDto};
use nuomi_shell_lib::state::AppState;
use nuomi_shell_lib::IpcError;
use serde_json::json;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn boot_with_memory_secrets() -> (AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let state = AppState::boot_with_secrets(
        db,
        ProviderSource::Fake(vec![ChatResponse::default()]),
        Arc::new(MemorySecretStore::default()),
        dir.path().join("ws"),
    )
    .await
    .unwrap();
    (state, dir)
}

fn error_code(err: IpcError) -> String {
    match err {
        IpcError::Generic { code, .. } => code.to_string(),
    }
}

fn fixture_path() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../crates/nuomi-core/tests/fixtures/fake_cli.js")
        .to_string_lossy()
        .into_owned()
}

fn role_input(name: &str, params: serde_json::Value) -> RoleInput {
    RoleInput {
        name: name.into(),
        provider_id: None,
        provider_ids: vec![],
        system_prompt_override: Some(format!("You are {name}, the specialist.")),
        tool_allowlist: vec!["fs.read".into()],
        required_capabilities: vec![],
        temperature: Some(0.3),
        max_tokens: Some(512),
        params,
    }
}

/// Seeds one enabled CLI agent profile through the IPC layer and returns it.
async fn seed_fixture_profile(state: &AppState, name: &str) -> commands::AgentProfileDto {
    commands::impl_upsert_agent_profile(
        state,
        commands::AgentProfileInput {
            name: name.into(),
            flavor: commands::CliFlavorDto::Plain,
            command: "node".into(),
            args: vec![fixture_path(), "plain".into()],
            env: Default::default(),
            working_dir: None,
            enabled: true,
        },
    )
    .await
    .unwrap()
}

// ---------------------------------------------------------------- CRUD

#[tokio::test]
async fn roles_and_teams_crud_roundtrip_with_member_validation() {
    let (state, _dir) = boot_with_memory_secrets().await;

    // Role upsert-insert then upsert-update keeps id/created_at.
    let r1 = commands::impl_upsert_role(&state, role_input("coder", json!({})))
        .await
        .unwrap();
    assert_eq!(r1.name, "coder");
    let r1b = commands::impl_upsert_role(
        &state,
        RoleInput {
            system_prompt_override: None,
            temperature: None,
            ..role_input("coder", json!({ "agent_profile_id": "later" }))
        },
    )
    .await
    .unwrap();
    assert_eq!(r1b.id, r1.id);
    assert_eq!(r1b.created_at, r1.created_at);
    assert_eq!(r1b.system_prompt_override, None);
    assert_eq!(r1b.params["agent_profile_id"], "later");

    let listed = commands::impl_list_roles(&state).await.unwrap();
    assert!(listed.iter().any(|r| r.id == r1.id));

    // Empty membership and unknown members both fail with team.member_missing.
    let empty = commands::impl_upsert_team(
        &state,
        TeamInput {
            name: "empty crew".into(),
            topology: TeamTopologyDto::Pipeline,
            member_role_ids: vec![],
            config: json!({}),
        },
    )
    .await;
    assert_eq!(error_code(empty.unwrap_err()), "team.member_missing");

    let ghost = commands::impl_upsert_team(
        &state,
        TeamInput {
            name: "ghost crew".into(),
            topology: TeamTopologyDto::GroupChat,
            member_role_ids: vec![r1.id.clone(), "no-such-role".into()],
            config: json!({}),
        },
    )
    .await;
    assert_eq!(error_code(ghost.unwrap_err()), "team.member_missing");

    // Valid team roundtrips with its topology and member order intact.
    let t1 = commands::impl_upsert_team(
        &state,
        TeamInput {
            name: "crew".into(),
            topology: TeamTopologyDto::Pipeline,
            member_role_ids: vec![r1.id.clone()],
            config: json!({ "max_rounds": 2 }),
        },
    )
    .await
    .unwrap();
    assert_eq!(t1.topology, TeamTopologyDto::Pipeline);
    assert_eq!(t1.member_role_ids, vec![r1.id.clone()]);
    assert_eq!(t1.config["max_rounds"], 2);

    let listed = commands::impl_list_teams(&state).await.unwrap();
    assert!(listed.iter().any(|t| t.id == t1.id));

    // Deletes report misses with stable codes.
    assert_eq!(
        error_code(
            commands::impl_delete_role(&state, "nope".into())
                .await
                .unwrap_err()
        ),
        "role.not_found"
    );
    assert_eq!(
        error_code(
            commands::impl_delete_team(&state, "nope".into())
                .await
                .unwrap_err()
        ),
        "team.not_found"
    );
    commands::impl_delete_role(&state, r1.id).await.unwrap();
    commands::impl_delete_team(&state, t1.id).await.unwrap();
}

// ------------------------------------------------- run_team_on_task E2E

#[tokio::test]
async fn run_team_on_task_drives_run_lifecycle_and_whiteboard() {
    let (state, _dir) = boot_with_memory_secrets().await;

    let profile = seed_fixture_profile(&state, "fixture executor").await;
    let exec_role = commands::impl_upsert_role(
        &state,
        role_input("executor", json!({ "agent_profile_id": profile.id })),
    )
    .await
    .unwrap();
    let team = commands::impl_upsert_team(
        &state,
        TeamInput {
            name: "cli crew".into(),
            topology: TeamTopologyDto::Pipeline,
            member_role_ids: vec![exec_role.id.clone()],
            config: json!({}),
        },
    )
    .await
    .unwrap();

    let task =
        commands::impl_create_task(&state, "produce the spec".into(), "via the cli crew".into())
            .await
            .unwrap();

    let run = commands::impl_run_team_on_task(&state, task.id.clone(), team.id.clone())
        .await
        .unwrap();

    // The background executor settles the run into a terminal state.
    let mut final_status = String::new();
    for _ in 0..600 {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let current = commands::impl_get_run(&state, run.id.clone())
            .await
            .unwrap();
        final_status = current.status;
        if final_status == "succeeded" || final_status == "failed" {
            break;
        }
    }
    assert_eq!(final_status, "succeeded");

    // state_changed sequence was persisted first, in machine order.
    let conn = rusqlite::Connection::open(state.db_path.to_string()).unwrap();
    let events = repos::events::list_by_aggregate(&conn, "run", &run.id, None).unwrap();
    let transitions: Vec<String> = events
        .iter()
        .filter(|e| e.kind == "state_changed")
        .filter_map(|e| e.payload["to"].as_str().map(str::to_string))
        .collect();
    assert_eq!(transitions, vec!["running", "succeeded"]);

    // The pipeline turn landed on the session whiteboard.
    let notes = commands::impl_list_whiteboard_notes(&state, run.session_id.clone())
        .await
        .unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0].author_role_id.as_deref(),
        Some(exec_role.id.as_str())
    );
    assert!(
        notes[0].body.contains("plain says"),
        "unexpected note body: {}",
        notes[0].body
    );

    // Task status validation: done tasks cannot start a team run.
    commands::impl_update_task_status(&state, task.id.clone(), "done".into())
        .await
        .unwrap();
    let rejected = commands::impl_run_team_on_task(&state, task.id, team.id).await;
    assert_eq!(error_code(rejected.unwrap_err()), "task.invalid_status");

    let missing_task =
        commands::impl_run_team_on_task(&state, "ghost-task".into(), "ghost-team".into()).await;
    assert_eq!(error_code(missing_task.unwrap_err()), "task.not_found");
}

#[tokio::test]
async fn run_team_on_task_reports_failure_into_terminal_event() {
    let (state, _dir) = boot_with_memory_secrets().await;

    let role = commands::impl_upsert_role(&state, role_input("lonely", json!({})))
        .await
        .unwrap();
    // Team references a role that gets deleted afterwards → MemberNotFound
    // surfaces as a failed terminal state carrying the error message.
    let team = commands::impl_upsert_team(
        &state,
        TeamInput {
            name: "doomed crew".into(),
            topology: TeamTopologyDto::Pipeline,
            member_role_ids: vec![role.id.clone()],
            config: json!({}),
        },
    )
    .await
    .unwrap();
    commands::impl_delete_role(&state, role.id).await.unwrap();

    let task = commands::impl_create_task(&state, "impossible".into(), String::new())
        .await
        .unwrap();
    let run = commands::impl_run_team_on_task(&state, task.id, team.id)
        .await
        .unwrap();

    let mut final_status = String::new();
    for _ in 0..600 {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let current = commands::impl_get_run(&state, run.id.clone())
            .await
            .unwrap();
        final_status = current.status;
        if final_status == "succeeded" || final_status == "failed" {
            break;
        }
    }
    assert_eq!(final_status, "failed");

    let conn = rusqlite::Connection::open(state.db_path.to_string()).unwrap();
    let events = repos::events::list_by_aggregate(&conn, "run", &run.id, None).unwrap();
    let failed = events
        .iter()
        .find(|e| e.kind == "state_changed" && e.payload["to"] == "failed")
        .expect("failed transition persisted");
    assert!(
        failed.payload["error"]
            .as_str()
            .is_some_and(|m| m.contains("not found")),
        "error message missing from payload: {}",
        failed.payload
    );
}

// ------------------------------------------------- run cancellation (AC4)

/// AC4: a board transition to `cancelled` stops the in-flight background
/// team run. The alive-mode CLI fixture never exits on its own and
/// heartbeats into a tempdir marker file (delivered through the profile's
/// `env`, which `CliAgentClient` forwards to the child), so the run window
/// is unbounded and the cancel race is deterministic. Asserts the terminal
/// `running→cancelled` transition with the `state_changed` event persisted
/// first (iron rule) and that the reaped child stops heartbeating.
#[tokio::test]
async fn cancel_team_run_transitions_to_cancelled() {
    let (state, dir) = boot_with_memory_secrets().await;

    let alive_path = dir.path().join("alive.txt");
    let profile = commands::impl_upsert_agent_profile(
        &state,
        commands::AgentProfileInput {
            name: "alive executor".into(),
            flavor: commands::CliFlavorDto::Plain,
            command: "node".into(),
            args: vec![fixture_path(), "alive".into()],
            env: BTreeMap::from([(
                "FAKE_CLI_ALIVE_FILE".into(),
                alive_path.to_string_lossy().into_owned(),
            )]),
            working_dir: None,
            enabled: true,
        },
    )
    .await
    .unwrap();
    let exec_role = commands::impl_upsert_role(
        &state,
        role_input("eternal", json!({ "agent_profile_id": profile.id })),
    )
    .await
    .unwrap();
    let team = commands::impl_upsert_team(
        &state,
        TeamInput {
            name: "endless crew".into(),
            topology: TeamTopologyDto::Pipeline,
            member_role_ids: vec![exec_role.id],
            config: json!({}),
        },
    )
    .await
    .unwrap();

    let task = commands::impl_create_task(&state, "never ends".into(), "until cancelled".into())
        .await
        .unwrap();
    let run = commands::impl_run_team_on_task(&state, task.id.clone(), team.id)
        .await
        .unwrap();

    fn file_len(path: &Path) -> u64 {
        std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
    }

    // Wait for the alive child to actually start heartbeating so the
    // post-cancel stability window below has a meaningful baseline.
    let mut heartbeat_len = 0u64;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        heartbeat_len = file_len(&alive_path);
        if heartbeat_len > 0 {
            break;
        }
    }
    assert!(heartbeat_len > 0, "alive fixture never started");

    // Existing cancellation entry point: board status → cancelled triggers
    // the run cancel registry for this task.
    commands::impl_update_task_status(&state, task.id.clone(), "cancelled".into())
        .await
        .unwrap();

    // Poll until the run settles cancelled (≤10s); it must never pass
    // through another terminal state on the way.
    let mut observed: Vec<String> = Vec::new();
    let mut final_status = String::new();
    for _ in 0..500 {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let current = commands::impl_get_run(&state, run.id.clone())
            .await
            .unwrap();
        if observed.last() != Some(&current.status) {
            observed.push(current.status.clone());
        }
        final_status = current.status;
        if final_status == "cancelled" {
            break;
        }
    }
    assert_eq!(final_status, "cancelled", "observed: {observed:?}");
    assert!(
        observed
            .iter()
            .all(|s| matches!(s.as_str(), "queued" | "running" | "cancelled")),
        "run passed through an unexpected state: {observed:?}"
    );

    let conn = rusqlite::Connection::open(state.db_path.to_string()).unwrap();

    // The task row itself landed on cancelled.
    let task_row = repos::tasks_runs::get_task(&conn, &task.id).unwrap();
    assert_eq!(task_row.status, nuomi_core::domain::TaskStatus::Cancelled);

    // Iron rule evidence: both transitions persisted as state_changed events
    // in machine order, and the terminal event was written no later than the
    // run-row update.
    let events = repos::events::list_by_aggregate(&conn, "run", &run.id, None).unwrap();
    let changed: Vec<&nuomi_core::domain::EventRecord> = events
        .iter()
        .filter(|e| e.kind == "state_changed")
        .collect();
    let transitions: Vec<String> = changed
        .iter()
        .filter_map(|e| e.payload["to"].as_str().map(str::to_string))
        .collect();
    assert_eq!(transitions, vec!["running", "cancelled"]);
    let terminal = changed.last().expect("terminal state_changed persisted");
    assert_eq!(terminal.payload["from"], "running");
    assert_eq!(terminal.payload["to"], "cancelled");
    let run_row = repos::tasks_runs::get_run(&conn, &run.id).unwrap();
    assert_eq!(run_row.status, nuomi_core::domain::RunState::Cancelled);
    assert!(
        terminal.created_at <= run_row.updated_at,
        "state_changed must be persisted before the row update"
    );

    // The kill-on-drop child was reaped: the heartbeat file stops growing
    // (3 consecutive stable samples at 150ms ≈ 450ms, comfortably longer
    // than the fixture's 100ms tick).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut last = file_len(&alive_path);
    let mut stable_samples = 0;
    while std::time::Instant::now() < deadline && stable_samples < 3 {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
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
        "alive heartbeat kept growing after cancellation (child not reaped): {last} bytes"
    );
}

// --------------------------------------------------- run_team_session

#[tokio::test]
async fn run_team_session_returns_outcome_directly() {
    let (state, _dir) = boot_with_memory_secrets().await;

    let profile = seed_fixture_profile(&state, "fixture executor").await;
    let exec_role = commands::impl_upsert_role(
        &state,
        role_input("executor", json!({ "agent_profile_id": profile.id })),
    )
    .await
    .unwrap();
    let team = commands::impl_upsert_team(
        &state,
        TeamInput {
            name: "session crew".into(),
            topology: TeamTopologyDto::Pipeline,
            member_role_ids: vec![exec_role.id],
            config: json!({}),
        },
    )
    .await
    .unwrap();

    let session_id = state.kernel.session_id().await;
    let outcome =
        commands::impl_run_team_session(&state, session_id.clone(), team.id, "say hello".into())
            .await
            .unwrap();
    assert!(outcome.converged);
    assert_eq!(outcome.rounds, 1);
    // The plain fixture echoes its prompt's first line prefixed with
    // "plain says:" — proof the CLI member executed (not an HTTP provider).
    assert!(
        outcome.final_output.contains("plain says"),
        "final output: {}",
        outcome.final_output
    );

    // No tasks_runs row is created on the session path (SPEC D5).
    let db = Db::open(&state.db_path).unwrap();
    migrations::run(&db.0).unwrap();
    let runs = repos::tasks_runs::list_runs_by_status(&db.0, nuomi_core::domain::RunState::Running)
        .unwrap();
    assert!(runs.is_empty());

    // Unknown team → stable error code from the orchestrator mapping.
    let err = commands::impl_run_team_session(
        &state,
        session_id,
        "no-such-team".into(),
        "whatever".into(),
    )
    .await;
    assert_eq!(error_code(err.unwrap_err()), "team.invalid_config");
}

// ------------------------------------------------------ whiteboard list

#[tokio::test]
async fn whiteboard_notes_list_in_seq_order() {
    let (state, _dir) = boot_with_memory_secrets().await;

    let session_id = state.kernel.session_id().await;
    let wb = nuomi_core::orchestrator::WhiteBoardService::new(state.db_path.clone());
    for body in ["third", "first", "second"] {
        wb.post(&session_id, None, "finding", body.into(), json!({}))
            .await
            .unwrap();
    }

    let notes = commands::impl_list_whiteboard_notes(&state, session_id)
        .await
        .unwrap();
    let seqs: Vec<i64> = notes.iter().map(|n| n.seq).collect();
    let mut sorted = seqs.clone();
    sorted.sort_unstable();
    assert_eq!(seqs, sorted, "notes must come back ordered by seq ASC");
    assert_eq!(seqs, vec![1, 2, 3]);
}

// --------------------------------------------- team formation (M-FORM1 F3)

/// Mock OpenAI-compatible endpoint answering every call with `content`.
async fn canned_server(content: impl Into<String>) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": { "role": "assistant", "content": content.into() },
                "finish_reason": "stop"
            }]
        })))
        .mount(&server)
        .await;
    server
}

/// Provider row whose key resolves through the documented
/// `NUOMI_PROVIDER_<ID>_API_KEY` env fallback (keyring_ref stays `None`).
fn env_keyed_provider(id: &str, name: &str, base_url: &str, master: bool) -> ProviderConfig {
    ProviderConfig {
        id: id.into(),
        name: name.into(),
        protocol: ProviderProtocol::OpenAiCompatible,
        base_url: base_url.into(),
        keyring_ref: None,
        capabilities: vec![],
        is_master: master,
        fallback_order: None,
        params: json!({}),
        created_at: 1,
        updated_at: 1,
    }
}

/// Env-var name for a provider id (naming convention frozen in
/// nuomi-core `team_runner::env_key_name`; unique ids keep parallel tests
/// from interfering).
fn provider_env_key(provider_id: &str) -> String {
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

fn open_conn(state: &AppState) -> rusqlite::Connection {
    rusqlite::Connection::open(state.db_path.to_string()).unwrap()
}

/// Polls the run to a terminal state and asserts it succeeded.
async fn poll_run_to_success(state: &AppState, run_id: &str) {
    let mut final_status = String::new();
    for _ in 0..600 {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        final_status = commands::impl_get_run(state, run_id.to_string())
            .await
            .unwrap()
            .status;
        if final_status == "succeeded" || final_status == "failed" {
            break;
        }
    }
    assert_eq!(final_status, "succeeded");
}

#[tokio::test]
async fn form_team_creates_team_then_runs_it_end_to_end() {
    let (state, _dir) = boot_with_memory_secrets().await;

    let profile = seed_fixture_profile(&state, "auto executor").await;
    let planner_id = "p-form-plan";
    let spec_id = "p-form-spec";

    // Reused writer role pinned to a loopback spec-writer endpoint.
    let spec_server = canned_server("spec ready").await;
    {
        let conn = open_conn(&state);
        repos::providers::insert_provider(
            &conn,
            &env_keyed_provider(spec_id, "spec writer", spec_server.uri().as_str(), false),
        )
        .unwrap();
        repos::roles::insert(
            &conn,
            &Role {
                id: "r-form-writer".into(),
                name: "writer".into(),
                provider_id: Some(spec_id.into()),
                provider_ids: vec![spec_id.into()],
                system_prompt_override: Some("You are the writer.".into()),
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
            },
        )
        .unwrap();
    }

    // Planner returns a valid plan reusing the role above plus one fresh
    // CLI-profile member.
    let plan_server = canned_server(format!(
        r#"{{"topology":"pipeline","members":[{{"kind":"role","id":"r-form-writer","roleName":"writer"}},{{"kind":"cli_profile","id":"{}","roleName":"executor","systemPrompt":"execute the plan"}}],"config":{{"maxRounds":4}},"rationale":"write then execute locally"}}"#,
        profile.id
    ))
    .await;
    {
        let conn = open_conn(&state);
        repos::providers::insert_provider(
            &conn,
            &env_keyed_provider(planner_id, "planner", plan_server.uri().as_str(), true),
        )
        .unwrap();
    }
    std::env::set_var(provider_env_key(planner_id), "dummy");
    std::env::set_var(provider_env_key(spec_id), "dummy");

    let session_id = state.kernel.session_id().await;
    let dto = commands::impl_form_team(&state, "produce the spec".into(), Some(session_id.clone()))
        .await
        .unwrap();

    assert_eq!(dto.member_role_ids.len(), 2);
    assert!(
        dto.name.starts_with("auto-"),
        "auto naming violated: {}",
        dto.name
    );
    assert_eq!(dto.topology, TeamTopologyDto::Pipeline);

    // The CLI member became a fresh role bound through the M-TEAM1 params
    // convention; the referenced writer role kept its fixed id.
    let new_role_id = dto
        .member_role_ids
        .iter()
        .find(|id| *id != "r-form-writer")
        .expect("created cli member listed in memberRoleIds")
        .clone();
    let conn = open_conn(&state);
    let created = repos::roles::get(&conn, &new_role_id).unwrap();
    assert_eq!(created.params["agent_profile_id"], profile.id.as_str());
    drop(conn);

    // Two-step UI flow (SPEC auto-team-m1 D7): feed the fresh team straight
    // into a board run.
    let task = commands::impl_create_task(
        &state,
        "produce the spec".into(),
        "via the auto-formed crew".into(),
    )
    .await
    .unwrap();
    let run = commands::impl_run_team_on_task(&state, task.id, dto.id)
        .await
        .unwrap();
    poll_run_to_success(&state, &run.id).await;
}

#[tokio::test]
async fn form_team_rejects_unparseable_plan_without_writes() {
    let (state, _dir) = boot_with_memory_secrets().await;

    let planner_id = "p-form-bad";
    let plan_server = canned_server("this is not json {").await;
    {
        let conn = open_conn(&state);
        repos::providers::insert_provider(
            &conn,
            &env_keyed_provider(planner_id, "planner", plan_server.uri().as_str(), false),
        )
        .unwrap();
    }
    std::env::set_var(provider_env_key(planner_id), "dummy");

    let conn = open_conn(&state);
    let roles_before = repos::roles::count(&conn).unwrap();
    let teams_before = repos::teams::count(&conn).unwrap();

    let err = commands::impl_form_team(&state, "impossible task".into(), None)
        .await
        .unwrap_err();
    assert_eq!(error_code(err), "team.plan_invalid");

    // Zero partial writes after both planner attempts failed.
    assert_eq!(repos::roles::count(&conn).unwrap(), roles_before);
    assert_eq!(repos::teams::count(&conn).unwrap(), teams_before);
}

#[tokio::test]
async fn form_team_rejects_blank_task() {
    let (state, _dir) = boot_with_memory_secrets().await;

    let err = commands::impl_form_team(&state, "   ".into(), None)
        .await
        .unwrap_err();
    assert_eq!(error_code(err), "task.invalid");
}

#[tokio::test]
async fn form_team_without_providers_fails_with_no_planner() {
    let (state, _dir) = boot_with_memory_secrets().await;

    let err = commands::impl_form_team(&state, "any task".into(), None)
        .await
        .unwrap_err();
    assert_eq!(error_code(err), "team.no_planner");

    let conn = open_conn(&state);
    assert_eq!(repos::roles::count(&conn).unwrap(), 0);
    assert_eq!(repos::teams::count(&conn).unwrap(), 0);
}

// ------------------------------------------------ team formation dry-run (打磨③a)

fn event_count(conn: &rusqlite::Connection, where_clause: &str) -> i64 {
    conn.query_row(
        &format!("SELECT count(*) FROM events {where_clause}"),
        [],
        |row| row.get(0),
    )
    .unwrap()
}

#[tokio::test]
async fn preview_team_reports_plan_without_writes_then_form_succeeds() {
    let (state, _dir) = boot_with_memory_secrets().await;

    let profile = seed_fixture_profile(&state, "auto preview executor").await;
    let planner_id = "p-preview-plan";
    let spec_id = "p-preview-spec";

    // Plan reusing a pre-built role, pinning a provider and binding the CLI
    // profile — all three member kinds in one shot.
    let plan_server = canned_server(format!(
        r#"{{"topology":"pipeline","members":[{{"kind":"role","id":"r-preview-writer","roleName":"writer"}},{{"kind":"provider","id":"{spec_id}","roleName":"reviewer"}},{{"kind":"cli_profile","id":"{}","roleName":"executor"}}],"config":{{"maxRounds":4,"required":["code"]}},"rationale":"draft, review, execute locally"}}"#,
        profile.id
    ))
    .await;
    {
        let conn = open_conn(&state);
        repos::providers::insert_provider(
            &conn,
            &env_keyed_provider(planner_id, "planner", plan_server.uri().as_str(), true),
        )
        .unwrap();
        repos::providers::insert_provider(
            &conn,
            &env_keyed_provider(spec_id, "spec writer", "http://localhost:9/v1", false),
        )
        .unwrap();
        repos::roles::insert(
            &conn,
            &Role {
                id: "r-preview-writer".into(),
                name: "writer".into(),
                provider_id: Some(spec_id.into()),
                provider_ids: vec![spec_id.into()],
                system_prompt_override: Some("You are the writer.".into()),
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
            },
        )
        .unwrap();
    }
    std::env::set_var(provider_env_key(planner_id), "dummy");

    let conn = open_conn(&state);
    let roles_before = repos::roles::count(&conn).unwrap();
    let teams_before = repos::teams::count(&conn).unwrap();
    let events_before = event_count(&conn, "");

    let plan = commands::impl_preview_team(&state, "produce the spec".into())
        .await
        .unwrap();

    assert_eq!(plan.topology, TeamTopologyDto::Pipeline);
    assert_eq!(plan.rationale, "draft, review, execute locally");
    assert_eq!(plan.max_rounds, Some(4));
    assert_eq!(plan.required, vec!["code".to_string()]);
    assert_eq!(plan.members.len(), 3);

    // Pre-built role: reused as-is, no creation flag.
    let writer = &plan.members[0];
    assert_eq!(writer.kind, "role");
    assert_eq!(writer.ref_id, "r-preview-writer");
    assert_eq!(writer.name, "writer");
    assert!(!writer.will_create_role);

    // Provider member would create a fresh pinned role on commit.
    let reviewer = &plan.members[1];
    assert_eq!(reviewer.kind, "provider");
    assert_eq!(reviewer.ref_id, spec_id);
    assert_eq!(reviewer.name, "reviewer");
    assert!(reviewer.will_create_role);

    // CLI profile member binds through the params convention.
    let executor = &plan.members[2];
    assert_eq!(executor.kind, "cli_profile");
    assert_eq!(executor.ref_id, profile.id);
    assert_eq!(executor.name, "executor");
    assert!(executor.will_create_role);

    // Zero writes: no role/team rows, no events at all (no team.formed).
    assert_eq!(repos::roles::count(&conn).unwrap(), roles_before);
    assert_eq!(repos::teams::count(&conn).unwrap(), teams_before);
    assert_eq!(event_count(&conn, ""), events_before);
    assert_eq!(event_count(&conn, "WHERE kind = 'team.formed'"), 0);
    drop(conn);

    // Zero bus events either: nothing on the kernel bus after the preview.
    let mut bus_rx = state.kernel.context().subscribe();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(bus_rx.try_recv().is_err(), "preview published a bus event");

    // Same inputs, real formation right after the preview: the shared
    // planning phase leaked nothing, so form succeeds and persists.
    let dto = commands::impl_form_team(&state, "produce the spec".into(), None)
        .await
        .unwrap();
    assert_eq!(dto.member_role_ids.len(), 3);

    let conn = open_conn(&state);
    assert_eq!(repos::roles::count(&conn).unwrap(), roles_before + 2);
    assert_eq!(repos::teams::count(&conn).unwrap(), teams_before + 1);

    // team.formed lives only on the bus (never in the events table): form
    // publishes exactly one, proving the event came from this call.
    assert_eq!(event_count(&conn, "WHERE kind = 'team.formed'"), 0);
    let formed = tokio::time::timeout(std::time::Duration::from_secs(5), bus_rx.recv())
        .await
        .expect("bus event within timeout")
        .expect("event received");
    assert_eq!(formed.topic, "team.formed");
    assert_eq!(
        formed.payload["memberCount"], 3,
        "unexpected payload: {}",
        formed.payload
    );

    // The created members mirror exactly what the preview promised.
    let reviewer_role = repos::roles::get(&conn, &dto.member_role_ids[1]).unwrap();
    assert_eq!(reviewer_role.name, reviewer.name);
    assert_eq!(reviewer_role.provider_id.as_deref(), Some(spec_id));
    let executor_role = repos::roles::get(&conn, &dto.member_role_ids[2]).unwrap();
    assert_eq!(executor_role.name, executor.name);
    assert_eq!(
        executor_role.params["agent_profile_id"],
        profile.id.as_str()
    );
}

#[tokio::test]
async fn preview_team_rejects_blank_task() {
    let (state, _dir) = boot_with_memory_secrets().await;

    let err = commands::impl_preview_team(&state, "   ".into())
        .await
        .unwrap_err();
    assert_eq!(error_code(err), "task.invalid");
}
