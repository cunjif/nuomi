//! Integration tests for session title auto-derivation (spec §5.1.1).
//!
//! Covers the bug fix where UI-created sessions stored an empty title `""`
//! while the derivation trigger only recognised the `DEFAULT_SESSION_TITLE`
//! placeholder. After the fix, `is_unnamed_title` treats both representations
//! as unnamed and both trigger auto-derivation on the first non-empty message.

use std::path::PathBuf;

use nuomi_core::domain::{ConversationKind, Session};
use nuomi_core::facade::{is_unnamed_title, NuomiConfig, NuomiKernel, DEFAULT_SESSION_TITLE};
use nuomi_core::providers::fake::FakeLlm;
use nuomi_core::store::{migrations, repos, Db};

fn fake_config(path: PathBuf) -> NuomiConfig {
    NuomiConfig::with_fake_provider(
        path,
        vec![
            FakeLlm::response("first answer"),
            FakeLlm::response("second answer"),
            FakeLlm::response("third answer"),
            FakeLlm::response("fourth answer"),
        ],
    )
}

fn insert_session(db_path: &std::path::Path, id: &str, title: &str) {
    let db = Db::open(&db_path.to_string_lossy()).unwrap();
    migrations::run(&db.0).unwrap();
    let now = 1700000000000i64;
    let session = Session {
        id: id.to_string(),
        title: title.to_string(),
        created_at: now,
        updated_at: now,
        kind: ConversationKind::Chat,
        team_id: None,
        task_id: None,
        schedule_id: None,
        goal: None,
        main_agent_id: None,
        route_mode: None,
        whiteboard_route_mode: None,
        deleted_at: None,
    };
    repos::sessions::insert(&db.0, &session).unwrap();
}

fn read_title(db_path: &std::path::Path, id: &str) -> String {
    let db = Db::open(&db_path.to_string_lossy()).unwrap();
    repos::sessions::get(&db.0, id).unwrap().title
}

#[tokio::test]
async fn ui_empty_title_session_derives_on_first_message() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("k.db");
    let kernel = NuomiKernel::boot(fake_config(db_path.clone()))
        .await
        .unwrap();

    // Simulate the UI path: create a session with an empty title (as
    // impl_create_conversation does via `title.as_deref().unwrap_or("")`).
    let sid = "ui-empty-title-session";
    insert_session(&db_path, sid, "");

    // Sanity: the session is unnamed per is_unnamed_title.
    assert!(is_unnamed_title(""));

    // Send the first non-empty message.
    kernel
        .run_task_in_session(sid, "hello world", None, None, None, None, None, None)
        .await
        .unwrap();

    // The title must be derived from the message, no longer empty.
    let title = read_title(&db_path, sid);
    assert!(!title.is_empty(), "title should be derived, got empty");
    assert_ne!(title, DEFAULT_SESSION_TITLE);
    assert!(!is_unnamed_title(&title));
}

#[tokio::test]
async fn cli_placeholder_title_session_derives_on_first_message() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("k.db");
    let kernel = NuomiKernel::boot(fake_config(db_path.clone()))
        .await
        .unwrap();

    // Simulate the CLI path: create a session with the placeholder title.
    let sid = "cli-placeholder-title-session";
    insert_session(&db_path, sid, DEFAULT_SESSION_TITLE);

    // Sanity: the placeholder is unnamed.
    assert!(is_unnamed_title(DEFAULT_SESSION_TITLE));

    kernel
        .run_task_in_session(sid, "first task", None, None, None, None, None, None)
        .await
        .unwrap();

    let title = read_title(&db_path, sid);
    assert!(!title.is_empty());
    assert_ne!(title, DEFAULT_SESSION_TITLE);
}

#[tokio::test]
async fn legacy_empty_title_session_derives_on_next_message() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("k.db");
    let kernel = NuomiKernel::boot(fake_config(db_path.clone()))
        .await
        .unwrap();

    // Simulate a legacy session row with an empty title (pre-fix UI data).
    let sid = "legacy-empty-title-session";
    insert_session(&db_path, sid, "");

    kernel
        .run_task_in_session(sid, "a new message", None, None, None, None, None, None)
        .await
        .unwrap();

    let title = read_title(&db_path, sid);
    assert!(!title.is_empty(), "legacy empty title should be derived");
}

#[tokio::test]
async fn user_named_session_is_protected_from_derivation() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("k.db");
    let kernel = NuomiKernel::boot(fake_config(db_path.clone()))
        .await
        .unwrap();

    let sid = "user-named-session";
    let user_title = "我的会话";
    insert_session(&db_path, sid, user_title);

    // Sanity: a user-named session is NOT unnamed.
    assert!(!is_unnamed_title(user_title));

    kernel
        .run_task_in_session(sid, "some message", None, None, None, None, None, None)
        .await
        .unwrap();

    // The user's title must be preserved.
    let title = read_title(&db_path, sid);
    assert_eq!(
        title, user_title,
        "user-named session must not be overwritten"
    );
}

#[tokio::test]
async fn second_message_does_not_rename_derived_session() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("k.db");
    let kernel = NuomiKernel::boot(fake_config(db_path.clone()))
        .await
        .unwrap();

    let sid = "derive-then-second-msg";
    insert_session(&db_path, sid, "");

    // First message derives the title.
    kernel
        .run_task_in_session(
            sid,
            "first message content",
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let title_after_first = read_title(&db_path, sid);
    assert!(
        !title_after_first.is_empty(),
        "first message should derive a title"
    );

    // Second message must not rename the session.
    kernel
        .run_task_in_session(
            sid,
            "second different content",
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap();
    let title_after_second = read_title(&db_path, sid);
    assert_eq!(
        title_after_first, title_after_second,
        "second message must not rename the session"
    );
}

#[tokio::test]
async fn whitespace_first_message_preserves_unnamed_state() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("k.db");
    let kernel = NuomiKernel::boot(fake_config(db_path.clone()))
        .await
        .unwrap();

    let sid = "whitespace-first-msg";
    insert_session(&db_path, sid, "");

    // Send a whitespace-only first message.
    kernel
        .run_task_in_session(sid, "   \n\t  ", None, None, None, None, None, None)
        .await
        .unwrap();

    // The title must remain unnamed (empty in this case).
    let title = read_title(&db_path, sid);
    assert!(
        is_unnamed_title(&title),
        "whitespace first message must preserve unnamed state, got {:?}",
        title
    );
}
