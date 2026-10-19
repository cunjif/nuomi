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
    let result = commands::run_conversation_turn(&state, &s1.id, "hello", None)
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
    commands::impl_write_file(&state, "a/b.txt".into(), "hello".into(), None)
        .await
        .unwrap();
    assert_eq!(
        commands::impl_read_file(&state, "a/b.txt".into(), None)
            .await
            .unwrap(),
        "hello"
    );
    let listing = commands::impl_list_dir(&state, "a".into(), None)
        .await
        .unwrap();
    assert_eq!(listing.len(), 1);

    let err = commands::impl_read_file(&state, "../escape.txt".into(), None).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn workspace_file_ops_roundtrip() {
    let (state, dir) = boot(vec![]).await;
    state.switch_workspace(dir.path().to_path_buf()).unwrap();

    // create_file
    commands::impl_create_file(&state, "notes/a.txt".into(), "hi".into(), None)
        .await
        .unwrap();
    assert_eq!(
        commands::impl_read_file(&state, "notes/a.txt".into(), None)
            .await
            .unwrap(),
        "hi",
    );

    // create_dir
    commands::impl_create_dir(&state, "docs".into(), None)
        .await
        .unwrap();
    let root = commands::impl_list_dir(&state, "".into(), None)
        .await
        .unwrap();
    assert!(root.iter().any(|e| e.name == "docs" && e.is_dir));

    // rename
    commands::impl_rename(&state, "notes/a.txt".into(), "notes/b.txt".into(), None)
        .await
        .unwrap();
    assert_eq!(
        commands::impl_read_file(&state, "notes/b.txt".into(), None)
            .await
            .unwrap(),
        "hi",
    );

    // copy
    commands::impl_copy(&state, "notes/b.txt".into(), "docs/b.txt".into(), None)
        .await
        .unwrap();
    assert_eq!(
        commands::impl_read_file(&state, "docs/b.txt".into(), None)
            .await
            .unwrap(),
        "hi",
    );

    // delete
    commands::impl_delete(&state, "notes/b.txt".into(), None)
        .await
        .unwrap();
    assert!(commands::impl_read_file(&state, "notes/b.txt".into(), None)
        .await
        .is_err());
    // copy preserved
    assert_eq!(
        commands::impl_read_file(&state, "docs/b.txt".into(), None)
            .await
            .unwrap(),
        "hi",
    );
}

#[tokio::test]
async fn workspace_file_ops_reject_escape() {
    let (state, dir) = boot(vec![]).await;
    state.switch_workspace(dir.path().to_path_buf()).unwrap();
    commands::impl_create_file(&state, "safe.txt".into(), "x".into(), None)
        .await
        .unwrap();

    for rel in ["../out.txt", "/etc/passwd", "C:\\abs"] {
        assert!(
            commands::impl_create_file(&state, rel.into(), "x".into(), None)
                .await
                .is_err()
        );
        assert!(commands::impl_create_dir(&state, rel.into(), None)
            .await
            .is_err());
        assert!(commands::impl_delete(&state, rel.into(), None)
            .await
            .is_err());
        assert!(
            commands::impl_rename(&state, rel.into(), "safe.txt".into(), None)
                .await
                .is_err()
        );
        assert!(
            commands::impl_copy(&state, rel.into(), "safe.txt".into(), None)
                .await
                .is_err()
        );
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
    commands::impl_workspace_switch(state, entry.id.clone())
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

    // Close any open workspace to unfocus (single-open: at most one).
    if let Some(open) = commands::impl_workspace_get_open(&state).await.unwrap() {
        commands::impl_workspace_close(&state, open.id)
            .await
            .unwrap();
    }

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

    if let Some(open) = commands::impl_workspace_get_open(&state).await.unwrap() {
        commands::impl_workspace_close(&state, open.id)
            .await
            .unwrap();
    }

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

// ---------- two-level workspace commands (ADR-0021, task 6.5) ----------

#[tokio::test]
async fn workspace_v2_directory_lifecycle() {
    let (state, dir) = boot(vec![]).await;

    // Fresh boot: empty registry → get_open is null.
    assert!(commands::impl_workspace_get_open(&state)
        .await
        .unwrap()
        .is_none());

    // Missing path is rejected with the spec'd error code.
    let missing = dir.path().join("nope");
    let err = commands::impl_workspace_add_directory(&state, missing.to_string_lossy().to_string())
        .await
        .expect_err("missing directory must be rejected");
    assert_eq!(ipc_code(&err), "workspace.directory_missing");

    // Empty state + unowned path → fresh workspace with one directory, open.
    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let ws1 = commands::impl_workspace_add_directory(&state, alpha.to_string_lossy().to_string())
        .await
        .unwrap();
    assert!(ws1.is_open.unwrap());
    assert!(ws1.is_focused.unwrap());
    assert_eq!(ws1.label.as_deref(), Some("alpha"));
    assert_eq!(ws1.label_full.as_deref(), Some("alpha"));
    let dirs = ws1.directories.as_ref().unwrap();
    assert_eq!(dirs.len(), 1);
    assert_eq!(dirs[0].display_name, "alpha");
    assert!(dirs[0].available);
    assert!(dirs[0].mounted);
    assert!(dirs[0].is_active);
    // In-memory sandbox root synced to the first available directory.
    assert!(state.current_workspace().ends_with("alpha"));

    // get_open returns the open workspace.
    let open = commands::impl_workspace_get_open(&state)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(open.id, ws1.id);

    // Open workspace → the second directory joins it (label updates).
    let beta = dir.path().join("beta");
    std::fs::create_dir_all(&beta).unwrap();
    let ws1b = commands::impl_workspace_add_directory(&state, beta.to_string_lossy().to_string())
        .await
        .unwrap();
    assert_eq!(ws1b.id, ws1.id);
    assert_eq!(ws1b.label.as_deref(), Some("alpha-beta"));
    assert_eq!(ws1b.directories.as_ref().unwrap().len(), 2);
    // Adding a directory auto-focuses it (spec 5.5.1-3b): the active marker
    // and the in-memory root move to the fresh directory.
    let dirs_b = ws1b.directories.as_ref().unwrap();
    assert!(!dirs_b[0].is_active);
    assert!(dirs_b[1].is_active);
    assert!(state.current_workspace().ends_with("beta"));

    // Unmount/mount roundtrip reflects in get_open; the active marker moves
    // off the unmounted directory and mount auto-focuses it again.
    let beta_id = ws1b.directories.as_ref().unwrap()[1].id.clone();
    commands::impl_workspace_unmount_directory(&state, ws1.id.clone(), beta_id.clone())
        .await
        .unwrap();
    let open = commands::impl_workspace_get_open(&state)
        .await
        .unwrap()
        .unwrap();
    let open_dirs = open.directories.as_ref().unwrap();
    assert!(!open_dirs[1].mounted);
    assert!(open_dirs[0].is_active);
    assert!(!open_dirs[1].is_active);
    commands::impl_workspace_mount_directory(&state, ws1.id.clone(), beta_id.clone())
        .await
        .unwrap();
    let open = commands::impl_workspace_get_open(&state)
        .await
        .unwrap()
        .unwrap();
    let open_dirs = open.directories.as_ref().unwrap();
    assert!(open_dirs[1].mounted);
    assert!(open_dirs[1].is_active);

    // list_workspaces carries the derived label + directories (B6).
    let listed = commands::impl_list_workspaces(&state).await.unwrap();
    let entry = listed.iter().find(|w| w.id == ws1.id).unwrap();
    assert_eq!(entry.label.as_deref(), Some("alpha-beta"));
    assert_eq!(entry.label_full.as_deref(), Some("alpha-beta"));
    assert_eq!(entry.directories.as_ref().unwrap().len(), 2);
    assert!(entry.is_open.unwrap());

    // v2 close → empty state; a third path opens a second workspace.
    commands::impl_workspace_close(&state, ws1.id.clone())
        .await
        .unwrap();
    assert!(commands::impl_workspace_get_open(&state)
        .await
        .unwrap()
        .is_none());
    let gamma = dir.path().join("gamma");
    std::fs::create_dir_all(&gamma).unwrap();
    let ws2 = commands::impl_workspace_add_directory(&state, gamma.to_string_lossy().to_string())
        .await
        .unwrap();
    assert_ne!(ws2.id, ws1.id);
    assert_eq!(ws2.label.as_deref(), Some("gamma"));
    assert!(state.current_workspace().ends_with("gamma"));

    // Switch back: label preserved, root re-synced to the first directory
    // (the slot still references the outgoing workspace — free per-workspace
    // memory — so the chain falls through to alpha).
    let switched = commands::impl_workspace_switch(&state, ws1.id.clone())
        .await
        .unwrap();
    assert_eq!(switched.id, ws1.id);
    assert_eq!(switched.label.as_deref(), Some("alpha-beta"));
    assert!(switched.directories.as_ref().unwrap()[0].is_active);
    assert!(state.current_workspace().ends_with("alpha"));

    // Removing a non-last directory keeps the workspace open (label shrinks).
    let alpha_id = switched.directories.as_ref().unwrap()[0].id.clone();
    let r1 = commands::impl_workspace_remove_directory(&state, ws1.id.clone(), alpha_id)
        .await
        .unwrap();
    assert!(!r1.workspace_closed);
    let open = commands::impl_workspace_get_open(&state)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(open.label.as_deref(), Some("beta"));
    // The removed directory was the effective active one → the root falls
    // through to the remaining directory.
    assert!(state.current_workspace().ends_with("beta"));

    // Removing the last directory auto-closes the workspace.
    let r2 = commands::impl_workspace_remove_directory(&state, ws1.id.clone(), beta_id)
        .await
        .unwrap();
    assert!(r2.workspace_closed);
    assert!(commands::impl_workspace_get_open(&state)
        .await
        .unwrap()
        .is_none());

    // An emptied workspace cannot be opened or switched to.
    let err = commands::impl_workspace_switch(&state, ws1.id.clone())
        .await
        .expect_err("workspace without directories must be rejected");
    assert_eq!(ipc_code(&err), "workspace.no_directories");
}

#[tokio::test]
async fn workspace_v2_focus_directory() {
    let (state, dir) = boot(vec![]).await;

    // Two directories in one workspace; adding auto-focuses the fresh one.
    // The workspace row's root_path is a placeholder uuid (ADR-0021) — all
    // file commands must resolve through the directory chain regardless.
    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    std::fs::write(alpha.join("a.txt"), "a").unwrap();
    let _ws = commands::impl_workspace_add_directory(&state, alpha.to_string_lossy().to_string())
        .await
        .unwrap();
    let beta = dir.path().join("beta");
    std::fs::create_dir_all(&beta).unwrap();
    std::fs::write(beta.join("b.txt"), "b").unwrap();
    let ws = commands::impl_workspace_add_directory(&state, beta.to_string_lossy().to_string())
        .await
        .unwrap();
    let dirs = ws.directories.as_ref().unwrap();
    let (alpha_id, beta_id) = (dirs[0].id.clone(), dirs[1].id.clone());
    assert!(dirs[1].is_active);
    assert!(state.current_workspace().ends_with("beta"));

    // File commands resolve through the active directory: list "" → beta.
    let entries = commands::impl_list_dir(&state, "".to_string(), Some(ws.id.clone()))
        .await
        .unwrap();
    assert!(entries.iter().any(|e| e.name == "b.txt"));
    assert!(!entries.iter().any(|e| e.name == "a.txt"));

    // Focus back to alpha: root + FileTree re-point, DTO marks alpha.
    let focused = commands::impl_workspace_focus_directory(&state, ws.id.clone(), alpha_id)
        .await
        .unwrap();
    assert!(focused.directories.as_ref().unwrap()[0].is_active);
    assert!(state.current_workspace().ends_with("alpha"));
    let entries = commands::impl_list_dir(&state, "".to_string(), Some(ws.id.clone()))
        .await
        .unwrap();
    assert!(entries.iter().any(|e| e.name == "a.txt"));
    assert!(!entries.iter().any(|e| e.name == "b.txt"));

    // Focusing an unknown directory id is rejected with the spec'd code.
    let err = commands::impl_workspace_focus_directory(&state, ws.id.clone(), "nope".to_string())
        .await
        .expect_err("unknown directory must be rejected");
    assert_eq!(ipc_code(&err), "workspace.directory_not_found");

    // Focusing a directory of a non-open workspace is rejected.
    let err = commands::impl_workspace_focus_directory(&state, "other-ws".to_string(), beta_id)
        .await
        .expect_err("non-open workspace must be rejected");
    assert_eq!(ipc_code(&err), "workspace.not_found");

    // A directory deleted from disk cannot be focused (spec 5.5.3-1).
    let gamma = dir.path().join("gamma");
    std::fs::create_dir_all(&gamma).unwrap();
    let ws = commands::impl_workspace_add_directory(&state, gamma.to_string_lossy().to_string())
        .await
        .unwrap();
    let gamma_id = ws.directories.as_ref().unwrap()[2].id.clone();
    assert!(state.current_workspace().ends_with("gamma"));
    std::fs::remove_dir_all(&gamma).unwrap();
    let err = commands::impl_workspace_focus_directory(&state, ws.id.clone(), gamma_id)
        .await
        .expect_err("stale directory must be rejected");
    assert_eq!(ipc_code(&err), "workspace.directory_missing");
}

#[tokio::test]
async fn workspace_v2_recent_list_derived_labels() {
    let (state, dir) = boot(vec![]).await;

    // ws1 with two directories; opening it touches workspace_recent.
    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let ws1 = commands::impl_workspace_add_directory(&state, alpha.to_string_lossy().to_string())
        .await
        .unwrap();
    let beta = dir.path().join("beta");
    std::fs::create_dir_all(&beta).unwrap();
    commands::impl_workspace_add_directory(&state, beta.to_string_lossy().to_string())
        .await
        .unwrap();
    commands::impl_workspace_close(&state, ws1.id.clone())
        .await
        .unwrap();

    // A second workspace used afterwards.
    let gamma = dir.path().join("gamma");
    std::fs::create_dir_all(&gamma).unwrap();
    let ws2 = commands::impl_workspace_add_directory(&state, gamma.to_string_lossy().to_string())
        .await
        .unwrap();
    commands::impl_workspace_close(&state, ws2.id.clone())
        .await
        .unwrap();

    // Recent entries carry the derived label, not the placeholder uuid
    // root path (spec 5.5.1-2a): directory display names joined with `-`.
    let recent = commands::impl_get_recent_workspaces(&state, 10)
        .await
        .unwrap();
    let r1 = recent
        .iter()
        .find(|r| r.workspace_id == ws1.id)
        .expect("opening ws1 must touch workspace_recent");
    assert_eq!(r1.label.as_deref(), Some("alpha-beta"));
    assert_eq!(r1.label_full.as_deref(), Some("alpha-beta"));
    assert!(!r1.is_pinned);
    assert!(!r1.anomaly);
    assert!(r1.last_used_at > 0);
    let r2 = recent.iter().find(|r| r.workspace_id == ws2.id).unwrap();
    assert_eq!(r2.label.as_deref(), Some("gamma"));
    assert_eq!(r2.label_full.as_deref(), Some("gamma"));

    // Switch back to ws1 and strip both directories: the last removal
    // auto-closes the workspace, and the surviving recent entry loses its
    // label (empty derivation → None, the UI falls back to the workspace id).
    let switched = commands::impl_workspace_switch(&state, ws1.id.clone())
        .await
        .unwrap();
    let dirs = switched.directories.as_ref().unwrap();
    let (alpha_id, beta_id) = (dirs[0].id.clone(), dirs[1].id.clone());
    commands::impl_workspace_remove_directory(&state, ws1.id.clone(), alpha_id)
        .await
        .unwrap();
    let r = commands::impl_workspace_remove_directory(&state, ws1.id.clone(), beta_id)
        .await
        .unwrap();
    assert!(r.workspace_closed);

    let recent = commands::impl_get_recent_workspaces(&state, 10)
        .await
        .unwrap();
    let r1 = recent.iter().find(|r| r.workspace_id == ws1.id).unwrap();
    assert_eq!(r1.label, None);
    assert_eq!(r1.label_full, None);
}

#[tokio::test]
async fn workspace_v2_migration_anomaly_flags() {
    let (state, dir) = boot(vec![]).await;

    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let ws1 = commands::impl_workspace_add_directory(&state, alpha.to_string_lossy().to_string())
        .await
        .unwrap();
    commands::impl_workspace_close(&state, ws1.id.clone())
        .await
        .unwrap();
    let gamma = dir.path().join("gamma");
    std::fs::create_dir_all(&gamma).unwrap();
    let ws2 = commands::impl_workspace_add_directory(&state, gamma.to_string_lossy().to_string())
        .await
        .unwrap();
    commands::impl_workspace_close(&state, ws2.id.clone())
        .await
        .unwrap();

    // Seed a `migration_report` event on ws1 the same way migration 0036's
    // correction pass does (path_invalid issue).
    let db = nuomi_core::store::Db::open(&state.db_path).unwrap();
    let payload = serde_json::json!({
        "migration": "0036_work_directories",
        "directory_id": "placeholder",
        "abs_path": alpha.to_string_lossy().to_string(),
        "issue": "path_invalid",
    });
    nuomi_core::store::repos::events::append(
        &db.0,
        "workspace",
        &ws1.id,
        "migration_report",
        &payload,
        1,
    )
    .unwrap();

    // The recent list flags the anomalous workspace (spec 5.5.3-2c).
    let recent = commands::impl_get_recent_workspaces(&state, 10)
        .await
        .unwrap();
    let r1 = recent.iter().find(|r| r.workspace_id == ws1.id).unwrap();
    assert!(r1.anomaly);
    assert_eq!(r1.label.as_deref(), Some("alpha"));
    let r2 = recent.iter().find(|r| r.workspace_id == ws2.id).unwrap();
    assert!(!r2.anomaly);

    // list_workspaces carries the same flag; healthy rows are Some(false).
    let listed = commands::impl_list_workspaces(&state).await.unwrap();
    let e1 = listed.iter().find(|w| w.id == ws1.id).unwrap();
    assert_eq!(e1.anomaly, Some(true));
    let e2 = listed.iter().find(|w| w.id == ws2.id).unwrap();
    assert_eq!(e2.anomaly, Some(false));
}

#[tokio::test]
async fn layout_snapshot_split_mode_retired() {
    let (state, _dir) = boot(vec![]).await;

    // Setting "split" no longer produces a split snapshot (task 6.8,
    // ADR-0021 single-open): the command falls through to single and never
    // persists the split pair.
    commands::impl_set_layout_snapshot(&state, "split".into(), Some(["a".into(), "b".into()]))
        .await
        .unwrap();
    let snap = commands::impl_get_layout_snapshot(&state)
        .await
        .unwrap()
        .expect("snapshot must exist after set");
    assert_eq!(snap.mode, "single");
    assert_eq!(snap.split_workspace_ids, None);

    // Overview roundtrips normally.
    commands::impl_set_layout_snapshot(&state, "overview".into(), None)
        .await
        .unwrap();
    let snap = commands::impl_get_layout_snapshot(&state)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snap.mode, "overview");

    // Legacy `split` rows written by older builds are read back normalised
    // (single mode, no split pair) — read compatibility without erroring.
    let db = nuomi_core::store::Db::open(&state.db_path).unwrap();
    nuomi_core::store::repos::workspace_layout_snapshot::upsert(
        &db.0,
        &nuomi_core::store::repos::workspace_layout_snapshot::LayoutSnapshotRow {
            mode: nuomi_core::services::LayoutMode::Split,
            split_workspace_ids: Some(["a".into(), "b".into()]),
            focused_workspace_id: None,
            captured_at: 1,
        },
    )
    .unwrap();
    let snap = commands::impl_get_layout_snapshot(&state)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(snap.mode, "single");
    assert_eq!(snap.split_workspace_ids, None);
}

#[tokio::test]
async fn workspace_v2_conversation_filter_covers_all_directories() {
    let (state, dir) = boot(vec![]).await;

    // ws1 with two work directories (alpha + beta): sessions created during
    // either directory's activity window carry the same workspace id — the
    // ownership key is the workspace, not the directory.
    let alpha = dir.path().join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    let ws1 = commands::impl_workspace_add_directory(&state, alpha.to_string_lossy().to_string())
        .await
        .unwrap();
    let beta = dir.path().join("beta");
    std::fs::create_dir_all(&beta).unwrap();
    commands::impl_workspace_add_directory(&state, beta.to_string_lossy().to_string())
        .await
        .unwrap();
    commands::impl_workspace_close(&state, ws1.id.clone())
        .await
        .unwrap();

    // A second workspace opened afterwards.
    let gamma = dir.path().join("gamma");
    std::fs::create_dir_all(&gamma).unwrap();
    let ws2 = commands::impl_workspace_add_directory(&state, gamma.to_string_lossy().to_string())
        .await
        .unwrap();

    // Two sessions in ws1 (one per directory era) + one in ws2.
    let mk = |kind: &str, title: &str, ws: &str| commands::ConversationInput {
        kind: kind.to_string(),
        title: Some(title.to_string()),
        agent: None,
        team_id: None,
        workspace_id: ws.to_string(),
    };
    commands::impl_create_conversation(&state, mk("chat", "alpha-era session", &ws1.id))
        .await
        .unwrap();
    commands::impl_create_conversation(&state, mk("group", "beta-era session", &ws1.id))
        .await
        .unwrap();
    commands::impl_create_conversation(&state, mk("chat", "gamma session", &ws2.id))
        .await
        .unwrap();

    // Filtering by ws1 surfaces sessions from BOTH directories (spec
    // 5.5.1-7a) and nothing from ws2.
    let ws1_rows = commands::impl_list_conversations(&state, None, Some(ws1.id.clone()))
        .await
        .unwrap();
    let titles: Vec<&str> = ws1_rows.iter().map(|c| c.title.as_str()).collect();
    assert!(titles.contains(&"alpha-era session"));
    assert!(titles.contains(&"beta-era session"));
    assert_eq!(ws1_rows.len(), 2);

    // ws2 only sees its own session.
    let ws2_rows = commands::impl_list_conversations(&state, None, Some(ws2.id.clone()))
        .await
        .unwrap();
    assert_eq!(ws2_rows.len(), 1);
    assert_eq!(ws2_rows[0].title, "gamma session");

    // kind + workspace filters compose (kind is an in-memory filter on top
    // of the workspace-scoped query).
    let group_rows =
        commands::impl_list_conversations(&state, Some("group".to_string()), Some(ws1.id.clone()))
            .await
            .unwrap();
    assert_eq!(group_rows.len(), 1);
    assert_eq!(group_rows[0].title, "beta-era session");

    // No workspace filter → cross-workspace aggregation (ChatTabBar's
    // empty-state fallback).
    let all_rows = commands::impl_list_conversations(&state, None, None)
        .await
        .unwrap();
    assert!(all_rows.len() >= 3);
}
