//! Reproducer for issue workspace-tab-readwrite-fail:
//! 同一工作区新增工作目录后，原已打开工作目录的文件 tab 显示"读写失败"。
//!
//! Root-cause hypothesis under test: impl_workspace_add_directory sets the
//! newly added directory as the active directory; resolve_workspace_root then
//! resolves read_file/list_dir relative paths against the active directory's
//! abs_path. After adding dir B, a file tab opened under dir A (relative path
//! "notes.txt") is read against B's root and fails with InvalidPath.

use nuomi_core::providers::ChatResponse;
use nuomi_shell_lib::commands;
use nuomi_shell_lib::state::AppState;

async fn boot(script: Vec<ChatResponse>) -> (AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let state = AppState::boot(
        db,
        nuomi_core::facade::ProviderSource::Fake(script),
        dir.path().join("ws"),
    )
    .await
    .unwrap();
    (state, dir)
}

#[tokio::test]
async fn adding_directory_breaks_existing_tab_read() {
    let (state, dir) = boot(vec![]).await;

    // Two distinct work directories under the temp dir.
    let dir_a = dir.path().join("wsA");
    let dir_b = dir.path().join("wsB");
    std::fs::create_dir_all(&dir_a).unwrap();
    std::fs::create_dir_all(&dir_b).unwrap();
    let abs_a = dir_a.to_string_lossy().to_string();
    let abs_b = dir_b.to_string_lossy().to_string();

    // 1. Create a workspace with initial directory A → active = A.
    let ws = commands::impl_workspace_create(&state, "repro-ws".into(), Some(abs_a.clone()))
        .await
        .expect("create workspace with dir A");
    let ws_id = ws.id.clone();
    eprintln!(
        "[BUGFIX_REPRO_TRACE] created workspace id={} is_active={}",
        ws_id, ws.is_active
    );

    // Ensure this workspace is the active/open one before adding another dir.
    commands::impl_workspace_switch(&state, ws_id.clone())
        .await
        .expect("switch to created workspace");

    // 2. Open a file under A (relative path "notes.txt"), write content.
    commands::impl_write_file(
        &state,
        "notes.txt".into(),
        "content-from-A".into(),
        Some(ws_id.clone()),
    )
    .await
    .expect("write notes.txt under dir A");

    // 3. Read it back — succeeds while active directory is A.
    let content_before = commands::impl_read_file(&state, "notes.txt".into(), Some(ws_id.clone()))
        .await
        .expect("read notes.txt before adding dir B");
    eprintln!(
        "[BUGFIX_REPRO_TRACE] read before add-dir: {:?}",
        content_before
    );
    assert_eq!(content_before, "content-from-A");

    // 4. Add a second work directory B → backend sets active directory = B.
    let ws_after = commands::impl_workspace_add_directory(&state, abs_b.clone())
        .await
        .expect("add directory B");
    eprintln!(
        "[BUGFIX_REPRO_TRACE] after add-directory, ws id={} is_active={}",
        ws_after.id, ws_after.is_active
    );

    // 5. Read the SAME relative path "notes.txt" (the already-open tab from A).
    //    After the fix, read_file searches all mounted directories (active first)
    //    and finds the file under A even though the active directory is now B.
    let read_result =
        commands::impl_read_file(&state, "notes.txt".into(), Some(ws_id.clone())).await;
    eprintln!(
        "[BUGFIX_REPRO_TRACE] read after add-dir result: {:?}",
        read_result
            .as_ref()
            .map(|s| s.as_str())
            .map_err(|e| e.to_string())
    );

    // Fix verified: the A-origin tab remains readable after adding dir B.
    assert_eq!(
        read_result.expect("read after adding dir B should succeed — file is in dir A"),
        "content-from-A",
    );
    eprintln!("[BUGFIX_REPRO_TRACE] FIX VERIFIED: A-origin tab still readable after adding dir B");
}
