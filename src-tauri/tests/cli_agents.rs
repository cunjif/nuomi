//! Integration tests for the CLI-agent IPC surface (SPEC cli-agents-m1 C4).
//! Real kernel + tempdir SQLite + fake provider; probe uses the real `node`
//! binary (present on dev machines) so no network and no webview.

use std::collections::BTreeMap;

use nuomi_core::facade::ProviderSource;
use nuomi_core::providers::FakeLlm;
use nuomi_shell_lib::commands::{self, AgentProfileInput, CliFlavorDto};
use nuomi_shell_lib::{state::AppState, IpcError};

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

fn input(name: &str, command: &str) -> AgentProfileInput {
    AgentProfileInput {
        name: name.into(),
        flavor: CliFlavorDto::ClaudeCode,
        command: command.into(),
        args: vec!["-p".into(), "{prompt}".into()],
        env: BTreeMap::from([("NUOMI_TEST".to_string(), "1".to_string())]),
        working_dir: None,
        enabled: true,
    }
}

fn code_of(err: &IpcError) -> &'static str {
    match err {
        IpcError::Generic { code, .. } => code,
    }
}

#[tokio::test]
async fn upsert_is_idempotent_by_name_and_updates_in_place() {
    let (state, _dir) = boot().await;

    let first = commands::impl_upsert_agent_profile(&state, input("claude-main", "claude"))
        .await
        .unwrap();
    assert_eq!(first.adapter, "cli");
    assert_eq!(first.flavor, CliFlavorDto::ClaudeCode);
    assert_eq!(first.args, vec!["-p".to_string(), "{prompt}".to_string()]);
    assert_eq!(first.env.get("NUOMI_TEST").map(String::as_str), Some("1"));

    let listed = commands::impl_list_agent_profiles(&state).await.unwrap();
    assert_eq!(listed.len(), 1);

    // Same name again → same row updated: id preserved, fields refreshed.
    let mut second = input("claude-main", "codex");
    second.flavor = CliFlavorDto::Codex;
    second.args = vec!["exec".into()];
    second.env = BTreeMap::new();
    second.enabled = false;
    let updated = commands::impl_upsert_agent_profile(&state, second)
        .await
        .unwrap();
    assert_eq!(updated.id, first.id);
    assert_eq!(updated.command, "codex");
    assert_eq!(updated.flavor, CliFlavorDto::Codex);
    assert_eq!(updated.args, vec!["exec".to_string()]);
    assert!(updated.env.is_empty());
    assert!(!updated.enabled);

    let listed = commands::impl_list_agent_profiles(&state).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, first.id);
}

#[tokio::test]
async fn blank_name_or_command_is_invalid() {
    let (state, _dir) = boot().await;

    for bad in [input("   ", "claude"), input("x", "   ")] {
        let err = commands::impl_upsert_agent_profile(&state, bad)
            .await
            .expect_err("blank fields must be rejected");
        assert_eq!(code_of(&err), "agent_profile.invalid");
    }
    assert!(commands::impl_list_agent_profiles(&state)
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn check_delete_roundtrip_reports_node_version_then_not_found() {
    let (state, _dir) = boot().await;

    let created = commands::impl_upsert_agent_profile(&state, input("node-agent", "node"))
        .await
        .unwrap();

    let check = commands::impl_check_cli_agent(&state, created.id.clone())
        .await
        .unwrap();
    assert!(check.ok, "node --version must succeed on dev machines");
    assert!(check.error.is_none());
    let version_line = check.version_line.unwrap_or_default();
    assert!(!version_line.trim().is_empty());

    // Unknown profile id → structured not_found.
    let missing = commands::impl_check_cli_agent(&state, "ghost".into()).await;
    match &missing {
        Err(e) => assert_eq!(code_of(e), "agent_profile.not_found"),
        Ok(dto) => panic!("expected not_found error, got {dto:?}"),
    }

    commands::impl_delete_agent_profile(&state, created.id.clone())
        .await
        .unwrap();
    let listed = commands::impl_list_agent_profiles(&state).await.unwrap();
    assert!(listed.is_empty());

    // Second delete → structured not_found.
    let again = commands::impl_delete_agent_profile(&state, created.id).await;
    match &again {
        Err(e) => assert_eq!(code_of(e), "agent_profile.not_found"),
        Ok(()) => panic!("expected not_found error on double delete"),
    }
}
