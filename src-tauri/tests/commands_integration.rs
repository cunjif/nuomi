//! Integration tests for the command impl layer (AC7/AC8 data plane).
//! Real kernel + tempdir SQLite + fake provider; no webview.

use nuomi_core::facade::ProviderSource;
use nuomi_core::providers::{ChatResponse, FakeLlm};
use nuomi_shell_lib::commands;
use nuomi_shell_lib::state::AppState;

async fn boot(script: Vec<ChatResponse>) -> (AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let state = AppState::boot(db, ProviderSource::Fake(script))
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
    assert_eq!(listed.len(), 3); // boot creates one

    commands::impl_resume_session(&state, s1.id.clone())
        .await
        .unwrap();
    let result = commands::impl_submit_task(&state, s1.id.clone(), "hello".into())
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
    let _ = dir;
}

#[tokio::test]
async fn schedule_crud_validates_expression() {
    let (state, _dir) = boot(vec![]).await;

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
    let all = commands::impl_list_schedules(&state).await.unwrap();
    let found = all.iter().find(|s| s.id == good.id).unwrap();
    assert!(!found.enabled);

    commands::impl_delete_schedule(&state, good.id.clone())
        .await
        .unwrap();
    let all = commands::impl_list_schedules(&state).await.unwrap();
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
            let pending = commands::impl_list_pending_approvals(&state).await.unwrap();
            assert!(pending.iter().any(|p| p.id == id));
            commands::impl_resolve_approval(&state, id.clone(), true)
                .await
                .unwrap();
            let pending = commands::impl_list_pending_approvals(&state).await.unwrap();
            assert!(!pending.iter().any(|p| p.id == id));
        }
        other => panic!("expected approval requirement, got {other:?}"),
    }
}
