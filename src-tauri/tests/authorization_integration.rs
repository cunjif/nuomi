//! Integration tests for the authorization command group (chat_rich_bind
//! spec 5.4/6.3): real kernel + tempdir SQLite, no webview.

use nuomi_core::facade::ProviderSource;
use nuomi_core::providers::FakeLlm;
use nuomi_core::services::authorization::{
    global_hub, resolve_authorization, AuthorizationDecision,
};
use nuomi_core::store::migrations;
use nuomi_core::store::repos::file_authorizations::{self, AuthorizationOp};
use nuomi_shell_lib::commands;
use nuomi_shell_lib::state::AppState;

fn err_code(err: &nuomi_shell_lib::IpcError) -> &str {
    match err {
        nuomi_shell_lib::IpcError::Generic { code, .. } => code,
    }
}

async fn boot() -> (AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let state = AppState::boot(
        db,
        ProviderSource::Fake(vec![FakeLlm::response("hi")]),
        dir.path().join("ws"),
    )
    .await
    .unwrap();
    (state, dir)
}

fn seed_request(dir: &tempfile::TempDir, session_id: &str, role_id: &str) -> String {
    let conn = rusqlite::Connection::open(dir.path().join("t.db")).unwrap();
    migrations::run(&conn).unwrap();
    file_authorizations::insert_pending(
        &conn,
        session_id,
        role_id,
        AuthorizationOp::Write,
        "C:\\proj\\a.txt",
        120_000,
    )
    .unwrap()
    .id
}

#[tokio::test]
async fn resolve_authorization_grant_and_terminal_states() {
    let (state, dir) = boot().await;
    let session = commands::impl_create_session(&state).await.unwrap();
    conn_seed_role(&dir, "role_1");
    let id = seed_request(&dir, &session.id, "role_1");

    // grant_session resolves the row.
    commands::impl_resolve_authorization(
        &state,
        commands::ResolveAuthorizationInputDto {
            authorization_id: id.clone(),
            decision: "grant_session".into(),
        },
    )
    .await
    .unwrap();

    let listed = commands::impl_list_pending_authorizations(&state, session.id.clone())
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].state, "granted_session");
    assert_eq!(listed[0].role_display_name, "Researcher");
    assert_eq!(listed[0].op, "write");
    assert_eq!(listed[0].target_path, "C:\\proj\\a.txt");

    // Second resolve on the terminal row: already_resolved error code.
    let err = commands::impl_resolve_authorization(
        &state,
        commands::ResolveAuthorizationInputDto {
            authorization_id: id,
            decision: "deny".into(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err_code(&err), "authorization.already_resolved");
}

#[tokio::test]
async fn resolve_authorization_unknown_id_is_not_found() {
    let (state, _dir) = boot().await;
    let err = commands::impl_resolve_authorization(
        &state,
        commands::ResolveAuthorizationInputDto {
            authorization_id: "ghost".into(),
            decision: "deny".into(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err_code(&err), "authorization.not_found");
}

#[tokio::test]
async fn resolve_authorization_rejects_unknown_decision() {
    let (state, _dir) = boot().await;
    let err = commands::impl_resolve_authorization(
        &state,
        commands::ResolveAuthorizationInputDto {
            authorization_id: "x".into(),
            decision: "maybe".into(),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(err_code(&err), "authorization.invalid_decision");
}

#[tokio::test]
async fn set_agent_auth_policy_roundtrip_and_unset() {
    let (state, dir) = boot().await;
    conn_seed_role(&dir, "role_1");

    commands::impl_set_agent_auth_policy(&state, "role_1".into(), "deny".into())
        .await
        .unwrap();
    // Policy visible via the sweeper's data source.
    let conn = rusqlite::Connection::open(dir.path().join("t.db")).unwrap();
    let policy =
        nuomi_core::store::repos::agent_auth_policies::find_by_role(&conn, "role_1").unwrap();
    assert_eq!(
        policy,
        Some(nuomi_core::store::repos::agent_auth_policies::AgentAuthPolicyValue::Deny)
    );
    drop(conn);

    // unset removes the row.
    commands::impl_set_agent_auth_policy(&state, "role_1".into(), "unset".into())
        .await
        .unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("t.db")).unwrap();
    let policy =
        nuomi_core::store::repos::agent_auth_policies::find_by_role(&conn, "role_1").unwrap();
    assert_eq!(policy, None);
}

#[tokio::test]
async fn set_agent_auth_policy_unknown_role_is_not_found() {
    let (state, _dir) = boot().await;
    let err = commands::impl_set_agent_auth_policy(&state, "ghost".into(), "deny".into())
        .await
        .unwrap_err();
    assert_eq!(err_code(&err), "role.not_found");
}

#[tokio::test]
async fn sweeper_resolves_expired_by_policy_via_hub() {
    let (state, dir) = boot().await;
    let session = commands::impl_create_session(&state).await.unwrap();
    conn_seed_role(&dir, "role_1");
    // Insert a policy (authorize) AND an already-expired request.
    let conn = rusqlite::Connection::open(dir.path().join("t.db")).unwrap();
    nuomi_core::store::repos::agent_auth_policies::upsert(
        &conn,
        "role_1",
        nuomi_core::store::repos::agent_auth_policies::AgentAuthPolicyValue::Authorize,
    )
    .unwrap();
    let id = file_authorizations::insert_pending(
        &conn,
        &session.id,
        "role_1",
        AuthorizationOp::Write,
        "C:\\x.txt",
        -1_000,
    )
    .unwrap()
    .id;
    drop(conn);

    // Register a waiter as the gate would, then sweep.
    let hub = global_hub().clone();
    let rx = {
        // The register_waiter API is private; simulate via resolve path —
        // the sweeper's waker path is exercised by the core unit tests.
        // Here we verify the state transition itself.
        let _ = &hub;
        let db_path = dir.path().join("t.db").to_string_lossy().to_string();
        let resolved = nuomi_core::services::authorization::sweep_once(&db_path, &hub)
            .await
            .unwrap();
        assert_eq!(resolved, 1);
        None::<()>
    };
    let _ = rx;
    let _ = AuthorizationDecision::GrantOnce;
    let _ = resolve_authorization;

    let conn = rusqlite::Connection::open(dir.path().join("t.db")).unwrap();
    let stored = file_authorizations::find_by_id(&conn, &id)
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.state,
        file_authorizations::AuthorizationState::TimeoutDefault
    );
}

fn conn_seed_role(dir: &tempfile::TempDir, role_id: &str) {
    let conn = rusqlite::Connection::open(dir.path().join("t.db")).unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO roles (id, name, created_at, updated_at) VALUES (?1, 'Researcher', 1, 1)",
        [role_id],
    )
    .unwrap();
}
