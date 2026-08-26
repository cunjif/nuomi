//! Integration tests for task deletion (批次二②): cascade with runs +
//! approvals, running-task guard, and miss. Real kernel + tempdir SQLite;
//! no network, no webview.

use nuomi_core::domain::run_state::RunState;
use nuomi_core::domain::{Approval, Run, Task, TaskStatus};
use nuomi_core::facade::ProviderSource;
use nuomi_core::store::{migrations, repos, Db};
use nuomi_shell_lib::commands;
use nuomi_shell_lib::{state::AppState, IpcError};

async fn boot() -> (AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let state = AppState::boot(db, ProviderSource::Fake(vec![]))
        .await
        .unwrap();
    (state, dir)
}

fn code_of(err: &IpcError) -> &'static str {
    match err {
        IpcError::Generic { code, .. } => code,
    }
}

fn seed_task(db: &Db, id: &str, status: TaskStatus) {
    let now = 1i64;
    repos::tasks_runs::insert_task(
        &db.0,
        &Task {
            id: id.into(),
            session_id: None,
            title: format!("task {id}"),
            description: String::new(),
            status,
            created_at: now,
            updated_at: now,
        },
    )
    .unwrap();
}

fn seed_run(db: &Db, id: &str, task_id: &str, status: RunState) {
    let now = 1i64;
    repos::tasks_runs::insert_run(
        &db.0,
        &Run {
            id: id.into(),
            task_id: task_id.into(),
            session_id: "s1".into(),
            status,
            heartbeat_at: now,
            created_at: now,
            updated_at: now,
        },
    )
    .unwrap();
}

fn seed_approval(db: &Db, id: &str, run_id: &str) {
    let now = 1i64;
    repos::tasks_runs::insert_approval(
        &db.0,
        &Approval {
            id: id.into(),
            run_id: run_id.into(),
            tool_name: "fs.write".into(),
            arguments_json: r#"{"path":"a.txt"}"#.into(),
            decision: nuomi_core::domain::ApprovalDecision::Pending,
            decided_at: None,
            created_at: now,
        },
    )
    .unwrap();
}

#[tokio::test]
async fn delete_cascades_runs_and_approvals_in_one_shot() {
    let (state, _dir) = boot().await;

    {
        let db = Db::open(&state.db_path).unwrap();
        migrations::run(&db.0).unwrap();
        seed_task(&db, "t-del", TaskStatus::Queued);
        seed_run(&db, "r1", "t-del", RunState::Failed);
        seed_run(&db, "r2", "t-del", RunState::Succeeded);
        seed_approval(&db, "a1", "r1");
        // Sibling that must survive.
        seed_task(&db, "t-keep", TaskStatus::Queued);
        seed_run(&db, "r3", "t-keep", RunState::Queued);
    }

    commands::impl_delete_task(&state, "t-del".into())
        .await
        .unwrap();

    // Task + children are gone; the sibling chain survives.
    let db = Db::open(&state.db_path).unwrap();
    assert!(repos::tasks_runs::get_task(&db.0, "t-del").is_err());
    assert!(repos::tasks_runs::list_runs_by_task(&db.0, "t-del")
        .unwrap()
        .is_empty());
    assert!(repos::tasks_runs::get_approval(&db.0, "a1").is_err());
    assert_eq!(
        repos::tasks_runs::get_task(&db.0, "t-keep").unwrap().id,
        "t-keep"
    );
    assert_eq!(
        repos::tasks_runs::list_runs_by_task(&db.0, "t-keep").unwrap()[0].id,
        "r3"
    );
    drop(db);

    // The board list no longer carries the deleted task.
    let listed = commands::impl_list_tasks(&state, None).await.unwrap();
    assert!(listed.iter().all(|t| t.id != "t-del"));
}

#[tokio::test]
async fn delete_refuses_running_task_with_invalid_status() {
    let (state, _dir) = boot().await;

    {
        let db = Db::open(&state.db_path).unwrap();
        migrations::run(&db.0).unwrap();
        seed_task(&db, "t-run", TaskStatus::Running);
        seed_run(&db, "r1", "t-run", RunState::Running);
    }

    let err = commands::impl_delete_task(&state, "t-run".into())
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "task.invalid_status");

    // Refusal is non-destructive: the row stays put.
    let db = Db::open(&state.db_path).unwrap();
    assert_eq!(
        repos::tasks_runs::get_task(&db.0, "t-run").unwrap().id,
        "t-run"
    );
    assert_eq!(
        repos::tasks_runs::list_runs_by_task(&db.0, "t-run")
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn delete_unknown_task_reports_not_found() {
    let (state, _dir) = boot().await;

    let err = commands::impl_delete_task(&state, "ghost".into())
        .await
        .unwrap_err();
    assert_eq!(code_of(&err), "task.not_found");
}
