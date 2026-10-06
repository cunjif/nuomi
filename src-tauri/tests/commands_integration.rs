//! Integration tests for the command impl layer (AC7/AC8 data plane).
//! Real kernel + tempdir SQLite + fake provider; no webview.

use nuomi_core::facade::ProviderSource;
use nuomi_core::providers::{ChatResponse, FakeLlm};
use nuomi_shell_lib::commands;
use nuomi_shell_lib::state::AppState;

async fn boot(script: Vec<ChatResponse>) -> (AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let state = AppState::boot(db, ProviderSource::Fake(script), dir.path().join("ws"))
        .await
        .unwrap();
    (state, dir)
}

#[tokio::test]
async fn session_lifecycle_and_event_pagination() {
    let (state, _dir) = boot(vec![FakeLlm::response("hi")]).await;

    let s1 = commands::impl_create_session(&state).await.unwrap();
    commands::impl_create_session(&state).await.unwrap();
    let listed = commands::impl_list_sessions(&state).await.unwrap();
    assert_eq!(listed.len(), 2); // boot session is in __migrated__, new ones in active ws

    commands::impl_resume_session(&state, s1.id.clone())
        .await
        .unwrap();
    let result = commands::run_conversation_turn(&state, &s1.id, "hello")
        .await
        .unwrap();
    assert_eq!(result.final_text, "hi");

    let page = commands::impl_list_events(&state, s1.id.clone(), 0)
        .await
        .unwrap();
    assert!(page.iter().any(|e| e.kind == "message"));
    let last = page.last().unwrap().seq;
    let tail = commands::impl_list_events(&state, s1.id.clone(), last)
        .await
        .unwrap();
    assert!(tail.is_empty(), "gap recovery after last seq must be empty");
}

#[tokio::test]
async fn task_board_transition_auto_dispatches_run() {
    let (state, _dir) = boot(vec![FakeLlm::response("done")]).await;

    let task = commands::impl_create_task(&state, "refactor".into(), "do it".into())
        .await
        .unwrap();
    assert_eq!(task.status, "queued");

    // Drag to running → auto-dispatch; run settles asynchronously.
    commands::impl_update_task_status(&state, task.id.clone(), "running".into())
        .await
        .unwrap();

    let mut settled = false;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let runs = commands::impl_list_runs_by_task(&state, task.id.clone())
            .await
            .unwrap();
        if runs
            .iter()
            .any(|r| r.status == "succeeded" || r.status == "failed")
        {
            settled = true;
            break;
        }
    }
    assert!(settled, "auto-dispatched run should reach a terminal state");

    // Board shows done only after explicit transition; run list has entries.
    let runs = commands::impl_list_runs_by_task(&state, task.id)
        .await
        .unwrap();
    assert!(!runs.is_empty());
}

#[tokio::test]
async fn workspace_sandbox_rejects_escape() {
    let (state, dir) = boot(vec![]).await;
    state.switch_workspace(dir.path().to_path_buf()).unwrap();
    commands::impl_write_file(&state, "a/b.txt".into(), "hello".into())
        .await
        .unwrap();
    assert_eq!(
        commands::impl_read_file(&state, "a/b.txt".into())
            .await
            .unwrap(),
        "hello"
    );
    let listing = commands::impl_list_dir(&state, "a".into()).await.unwrap();
    assert_eq!(listing.len(), 1);

    let err = commands::impl_read_file(&state, "../escape.txt".into()).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn workspace_file_ops_roundtrip() {
    let (state, dir) = boot(vec![]).await;
    state.switch_workspace(dir.path().to_path_buf()).unwrap();

    // create_file
    commands::impl_create_file(&state, "notes/a.txt".into(), "hi".into())
        .await
        .unwrap();
    assert_eq!(
        commands::impl_read_file(&state, "notes/a.txt".into())
            .await
            .unwrap(),
        "hi",
    );

    // create_dir
    commands::impl_create_dir(&state, "docs".into())
        .await
        .unwrap();
    let root = commands::impl_list_dir(&state, "".into()).await.unwrap();
    assert!(root.iter().any(|e| e.name == "docs" && e.is_dir));

    // rename
    commands::impl_rename(&state, "notes/a.txt".into(), "notes/b.txt".into())
        .await
        .unwrap();
    assert_eq!(
        commands::impl_read_file(&state, "notes/b.txt".into())
            .await
            .unwrap(),
        "hi",
    );

    // copy
    commands::impl_copy(&state, "notes/b.txt".into(), "docs/b.txt".into())
        .await
        .unwrap();
    assert_eq!(
        commands::impl_read_file(&state, "docs/b.txt".into())
            .await
            .unwrap(),
        "hi",
    );

    // delete
    commands::impl_delete(&state, "notes/b.txt".into())
        .await
        .unwrap();
    assert!(commands::impl_read_file(&state, "notes/b.txt".into())
        .await
        .is_err());
    // copy preserved
    assert_eq!(
        commands::impl_read_file(&state, "docs/b.txt".into())
            .await
            .unwrap(),
        "hi",
    );
}

#[tokio::test]
async fn workspace_file_ops_reject_escape() {
    let (state, dir) = boot(vec![]).await;
    state.switch_workspace(dir.path().to_path_buf()).unwrap();
    commands::impl_create_file(&state, "safe.txt".into(), "x".into())
        .await
        .unwrap();

    for rel in ["../out.txt", "/etc/passwd", "C:\\abs"] {
        assert!(commands::impl_create_file(&state, rel.into(), "x".into())
            .await
            .is_err());
        assert!(commands::impl_create_dir(&state, rel.into()).await.is_err());
        assert!(commands::impl_delete(&state, rel.into()).await.is_err());
        assert!(commands::impl_rename(&state, rel.into(), "safe.txt".into())
            .await
            .is_err());
        assert!(commands::impl_copy(&state, rel.into(), "safe.txt".into())
            .await
            .is_err());
    }
}

#[tokio::test]
async fn schedule_crud_validates_expression() {
    let (state, _dir) = boot(vec![]).await;
    open_first_workspace(&state).await;

    let bad = commands::impl_create_schedule(
        &state,
        "bad".into(),
        "not a cron".into(),
        "t".into(),
        String::new(),
    )
    .await;
    assert!(bad.is_err());

    let good = commands::impl_create_schedule(
        &state,
        "daily".into(),
        "@every 3600".into(),
        "morning refactor".into(),
        String::new(),
    )
    .await
    .unwrap();
    assert!(good.enabled);

    commands::impl_toggle_schedule(&state, good.id.clone(), false)
        .await
        .unwrap();
    let all = commands::impl_list_schedules(&state, None).await.unwrap();
    let found = all.iter().find(|s| s.id == good.id).unwrap();
    assert!(!found.enabled);

    commands::impl_delete_schedule(&state, good.id.clone())
        .await
        .unwrap();
    let all = commands::impl_list_schedules(&state, None).await.unwrap();
    assert!(all.iter().all(|s| s.id != good.id || !s.enabled));
}

#[tokio::test]
async fn approval_flow_via_commands() {
    use nuomi_core::plugins::approval_gate::{self, SensitiveToolPolicy};
    use nuomi_core::plugins::MemoryService;

    let (state, _dir) = boot(vec![ChatResponse::default()]).await;
    let mem = MemoryService::new(state.db_path.clone());
    approval_gate::set_sensitive_tools(&mem, &["fs.write*".to_string()])
        .await
        .unwrap();

    let tools = commands::impl_get_sensitive_tools(&state).await.unwrap();
    assert_eq!(tools, Some(vec!["fs.write*".to_string()]));

    // Gate creates a pending row; inbox lists it; resolve flips decision.
    // A run row must exist (approvals.run_id FK).
    let now = 1i64;
    let db = nuomi_core::store::Db::open(&state.db_path).unwrap();
    nuomi_core::store::migrations::run(&db.0).unwrap();
    let task = nuomi_core::domain::Task {
        id: "t-approval".into(),
        session_id: None,
        title: "t".into(),
        description: String::new(),
        status: nuomi_core::domain::TaskStatus::Queued,
        created_at: now,
        updated_at: now,
        workspace_id: "__migrated__".into(),
    };
    let run = nuomi_core::domain::Run {
        id: "run-test".into(),
        task_id: task.id.clone(),
        session_id: "s".into(),
        status: nuomi_core::domain::run_state::RunState::Running,
        heartbeat_at: now,
        created_at: now,
        updated_at: now,
    };
    nuomi_core::store::repos::tasks_runs::insert_task(&db.0, &task).unwrap();
    nuomi_core::store::repos::tasks_runs::insert_run(&db.0, &run).unwrap();

    let gate = approval_gate::ApprovalGate::new(
        state.db_path.clone(),
        "run-test",
        SensitiveToolPolicy::new(["fs.write"]),
    );
    let decision = gate
        .check("fs.write", &serde_json::json!({"path": "x"}))
        .await
        .unwrap();
    match decision {
        approval_gate::GateDecision::RequireApproval { approval_id: id } => {
            let pending = commands::impl_list_pending_approvals(&state, None)
                .await
                .unwrap();
            assert!(pending.iter().any(|p| p.id == id));
            commands::impl_resolve_approval(&state, id.clone(), true)
                .await
                .unwrap();
            let pending = commands::impl_list_pending_approvals(&state, None)
                .await
                .unwrap();
            assert!(!pending.iter().any(|p| p.id == id));
        }
        other => panic!("expected approval requirement, got {other:?}"),
    }
}

// ---------- role capability system (presets / capability binding) ----------

fn ipc_code(err: &nuomi_shell_lib::IpcError) -> &'static str {
    match err {
        nuomi_shell_lib::IpcError::Generic { code, .. } => code,
    }
}

fn provider_input(
    id: Option<String>,
    caps: Vec<commands::CapabilityDto>,
) -> commands::ProviderInput {
    commands::ProviderInput {
        id,
        name: "vision-main".into(),
        protocol: commands::ProviderProtocolDto::OpenAiCompatible,
        base_url: "http://localhost:9/v1".into(),
        capabilities: vec![],
        is_master: false,
        api_key: None,
        settings: commands::ProviderSettingsDto {
            models: vec![commands::ModelEntryDto {
                id: "m-1".into(),
                capabilities: caps,
                temperature: None,
                top_p: None,
                max_tokens: None,
            }],
            ..Default::default()
        },
    }
}

#[tokio::test]
async fn role_capability_mismatch_and_preset_protection() {
    let (state, _dir) = boot(vec![]).await;

    // Provider carries reasoning only.
    commands::impl_upsert_provider(
        &state,
        provider_input(None, vec![commands::CapabilityDto::Reasoning]),
    )
    .await
    .unwrap();
    let providers = commands::impl_list_providers(&state).await.unwrap();
    let pid = providers.first().unwrap().id.clone();

    // Image is required but not covered → role.capability_mismatch with the
    // missing capability in details.
    let err = commands::impl_upsert_role(
        &state,
        commands::RoleInput {
            name: "vision".into(),
            provider_id: Some(pid.clone()),
            provider_ids: vec![pid.clone()],
            system_prompt_override: None,
            tool_allowlist: vec![],
            required_capabilities: vec![
                commands::CapabilityDto::Reasoning,
                commands::CapabilityDto::Image,
            ],
            temperature: None,
            max_tokens: None,
            params: serde_json::json!({}),
        },
    )
    .await
    .expect_err("must reject uncovered capability");
    assert_eq!(ipc_code(&err), "role.capability_mismatch");

    // Matching capabilities are accepted; provider_id stays synced.
    let role = commands::impl_upsert_role(
        &state,
        commands::RoleInput {
            name: "thinker".into(),
            provider_id: Some(pid.clone()),
            provider_ids: vec![pid],
            system_prompt_override: None,
            tool_allowlist: vec![],
            required_capabilities: vec![commands::CapabilityDto::Reasoning],
            temperature: None,
            max_tokens: None,
            params: serde_json::json!({}),
        },
    )
    .await
    .unwrap();
    assert_eq!(role.provider_ids.len(), 1);

    // Preset seeding is idempotent; built-ins refuse deletion.
    let first = commands::impl_seed_builtin_roles(&state).await.unwrap();
    assert!(first.inserted > 0);
    let again = commands::impl_seed_builtin_roles(&state).await.unwrap();
    assert_eq!(again.inserted, 0);
    assert_eq!(again.updated, first.inserted);

    let roles = commands::impl_list_roles(&state).await.unwrap();
    let builtin = roles.iter().find(|r| r.builtin).unwrap();
    let err = commands::impl_delete_role(&state, builtin.id.clone(), false)
        .await
        .expect_err("built-in roles are delete-protected");
    assert_eq!(ipc_code(&err), "role.builtin_protected");

    // User roles still delete normally.
    commands::impl_delete_role(&state, role.id, false)
        .await
        .unwrap();
}

// ---------- workspace-scoped view filter (task 7.2) ----------

/// Registers the boot workspace, opens it (focusing it), and returns its id.
async fn open_first_workspace(state: &AppState) -> String {
    let ws_path = state.current_workspace().to_string_lossy().to_string();
    let entry = commands::impl_add_workspace(state, ws_path).await.unwrap();
    commands::impl_open_workspace(state, entry.id.clone())
        .await
        .unwrap();
    entry.id
}

#[tokio::test]
async fn list_tasks_with_workspace_id_filters() {
    let (state, _dir) = boot(vec![]).await;
    let ws_id = open_first_workspace(&state).await;

    let t1 = commands::impl_create_task(&state, "in-ws".into(), String::new())
        .await
        .unwrap();
    assert_eq!(t1.workspace_id, ws_id);

    // Filtered by workspace → includes the task
    let filtered = commands::impl_list_tasks(&state, None, Some(ws_id.clone()))
        .await
        .unwrap();
    assert!(filtered.iter().any(|t| t.id == t1.id));

    // Filtered by non-existent workspace → empty
    let empty = commands::impl_list_tasks(&state, None, Some("ws-nope".into()))
        .await
        .unwrap();
    assert!(empty.is_empty());

    // No filter (None) → all tasks (backward compatible)
    let all = commands::impl_list_tasks(&state, None, None).await.unwrap();
    assert!(all.iter().any(|t| t.id == t1.id));
}

#[tokio::test]
async fn list_schedules_with_workspace_id_filters() {
    let (state, _dir) = boot(vec![]).await;
    let ws_id = open_first_workspace(&state).await;

    let s = commands::impl_create_schedule(
        &state,
        "daily".into(),
        "@every 3600".into(),
        "tick".into(),
        String::new(),
    )
    .await
    .unwrap();
    assert_eq!(s.workspace_id, ws_id);

    // Filtered by workspace → includes the schedule
    let filtered = commands::impl_list_schedules(&state, Some(ws_id.clone()))
        .await
        .unwrap();
    assert!(filtered.iter().any(|x| x.id == s.id));

    // Filtered by non-existent workspace → empty
    let empty = commands::impl_list_schedules(&state, Some("ws-nope".into()))
        .await
        .unwrap();
    assert!(empty.is_empty());

    // No filter → all schedules
    let all = commands::impl_list_schedules(&state, None).await.unwrap();
    assert!(all.iter().any(|x| x.id == s.id));
}

#[tokio::test]
async fn create_schedule_rejects_without_focused_workspace() {
    let (state, _dir) = boot(vec![]).await;

    // Close all workspaces to unfocus
    commands::impl_close_all_workspaces(&state, false)
        .await
        .unwrap();

    let err = commands::impl_create_schedule(
        &state,
        "orphan".into(),
        "@every 60".into(),
        "tick".into(),
        String::new(),
    )
    .await
    .expect_err("must reject without focused workspace");
    assert_eq!(ipc_code(&err), "scheduler.no_focused_workspace");
}

#[tokio::test]
async fn create_task_writes_migrated_placeholder_when_no_focused_workspace() {
    let (state, _dir) = boot(vec![]).await;

    commands::impl_close_all_workspaces(&state, false)
        .await
        .unwrap();

    let task = commands::impl_create_task(&state, "no-ws".into(), String::new())
        .await
        .unwrap();
    assert_eq!(task.workspace_id, "__migrated__");
}

#[tokio::test]
async fn view_scope_get_defaults_to_all() {
    let (state, _dir) = boot(vec![]).await;

    for surface in ["board", "approvals", "scheduler"] {
        let scope = commands::impl_get_view_scope(&state, surface.into())
            .await
            .unwrap();
        assert_eq!(scope, "all");
    }
}

#[tokio::test]
async fn view_scope_set_and_get_roundtrip() {
    let (state, _dir) = boot(vec![]).await;

    commands::impl_set_view_scope(&state, "board".into(), "focused".into())
        .await
        .unwrap();
    let got = commands::impl_get_view_scope(&state, "board".into())
        .await
        .unwrap();
    assert_eq!(got, "focused");

    // Different surface is independent
    let other = commands::impl_get_view_scope(&state, "approvals".into())
        .await
        .unwrap();
    assert_eq!(other, "all");

    // Toggle back
    commands::impl_set_view_scope(&state, "board".into(), "all".into())
        .await
        .unwrap();
    let back = commands::impl_get_view_scope(&state, "board".into())
        .await
        .unwrap();
    assert_eq!(back, "all");
}

#[tokio::test]
async fn view_scope_set_rejects_invalid_scope() {
    let (state, _dir) = boot(vec![]).await;
    let err = commands::impl_set_view_scope(&state, "board".into(), "bogus".into())
        .await
        .expect_err("invalid scope must be rejected");
    assert_eq!(ipc_code(&err), "view_scope.invalid_scope");
}

#[tokio::test]
async fn view_scope_rejects_invalid_surface() {
    let (state, _dir) = boot(vec![]).await;
    let err_get = commands::impl_get_view_scope(&state, "bogus".into())
        .await
        .expect_err("invalid surface must be rejected");
    assert_eq!(ipc_code(&err_get), "view_scope.invalid_surface");

    let err_set = commands::impl_set_view_scope(&state, "bogus".into(), "all".into())
        .await
        .expect_err("invalid surface must be rejected");
    assert_eq!(ipc_code(&err_set), "view_scope.invalid_surface");
}
