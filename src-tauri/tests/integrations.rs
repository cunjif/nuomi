//! M-BOT1 B3/B4 integration tests (SPEC docs/specs/bots-telemetry-m1):
//! integration CRUD over the IPC impl layer with URL masking across the
//! boundary, `test_integration` probe (valid Feishu payload / failing
//! endpoint), delete misses, and the notification-dispatcher smoke path
//! (bus event → whitelisted sink → wiremock). Loopback only, zero real
//! network.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use nuomi_core::facade::ProviderSource;
use nuomi_core::harness::Event;
use nuomi_core::providers::{ChatResponse, MemorySecretStore};
use nuomi_shell_lib::commands::{IntegrationInput, IntegrationKindDto};
use nuomi_shell_lib::{commands, notifier};
use serde_json::{json, Value};

type Captured = Arc<Mutex<Vec<Value>>>;

async fn boot_with_memory_secrets() -> (nuomi_shell_lib::state::AppState, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("t.db");
    let state = nuomi_shell_lib::state::AppState::boot_with_secrets(
        db,
        ProviderSource::Fake(vec![ChatResponse::default()]),
        Arc::new(MemorySecretStore::default()),
    )
    .await
    .unwrap();
    (state, dir)
}

fn error_code(err: nuomi_shell_lib::IpcError) -> String {
    match err {
        nuomi_shell_lib::IpcError::Generic { code, .. } => code.to_string(),
    }
}

fn feishu_input(name: &str, url: &str, events: Vec<String>) -> IntegrationInput {
    IntegrationInput {
        name: name.into(),
        kind: IntegrationKindDto::FeishuBot,
        webhook_url: url.into(),
        secret: Some("sign-secret".into()),
        headers: None,
        events,
        enabled: true,
    }
}

/// Feishu-style endpoint capturing bodies and answering the business-ok
/// envelope `{"code":0}`.
async fn capturing_feishu_server() -> (wiremock::MockServer, Captured) {
    let server = wiremock::MockServer::start().await;
    let captured: Captured = Arc::new(Mutex::new(Vec::new()));
    let sink = captured.clone();
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .respond_with(move |req: &wiremock::Request| {
            let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
            sink.lock().unwrap().push(body);
            wiremock::ResponseTemplate::new(200).set_body_json(json!({ "code": 0 }))
        })
        .mount(&server)
        .await;
    (server, captured)
}

// ------------------------------------------------- upsert / list / masking

#[tokio::test]
async fn upsert_and_list_mask_urls_and_upsert_is_idempotent_by_name() {
    let (state, _dir) = boot_with_memory_secrets().await;
    let raw = "https://open.feishu.cn/open-apis/bot/v2/hook/a1b2c3d4e5f6x9z8";

    let dto = commands::impl_upsert_integration(
        &state,
        feishu_input("feishu-main", raw, vec!["run.state_changed".into()]),
    )
    .await
    .unwrap();

    assert_eq!(dto.kind, IntegrationKindDto::FeishuBot);
    assert_eq!(dto.events, vec!["run.state_changed"]);
    assert!(dto
        .webhook_url_masked
        .starts_with("https://open.feishu.cn/"));
    assert!(dto.webhook_url_masked.ends_with("x9z8"));
    assert_ne!(dto.webhook_url_masked, raw);
    // Raw URL and secret never cross the IPC boundary.
    let serialized = serde_json::to_string(&dto).unwrap();
    assert!(!serialized.contains(raw), "raw URL leaked in DTO");
    assert!(!serialized.contains("sign-secret"), "secret leaked in DTO");

    // Idempotent update under the same name keeps id/created_at.
    let mut changed = feishu_input("feishu-main", raw, vec!["approval.requested".into()]);
    changed.enabled = false;
    let updated = commands::impl_upsert_integration(&state, changed)
        .await
        .unwrap();
    assert_eq!(updated.id, dto.id);
    assert_eq!(updated.created_at, dto.created_at);
    assert!(!updated.enabled);
    assert_eq!(updated.events, vec!["approval.requested"]);

    let listed = commands::impl_list_integrations(&state).await.unwrap();
    assert_eq!(listed.len(), 1, "same name must not create a second row");
    assert!(!serde_json::to_string(&listed).unwrap().contains(raw));

    // The row itself stores raw URL + feishu secret by design (write-only).
    let conn = rusqlite::Connection::open(state.db_path.to_string()).unwrap();
    let row = nuomi_core::store::repos::integrations::list(&conn)
        .unwrap()
        .remove(0);
    assert_eq!(row.config["webhook_url"], raw);
    assert_eq!(row.config["secret"], "sign-secret");

    // Invalid scheme rejected with the stable code.
    let err = commands::impl_upsert_integration(
        &state,
        feishu_input("bad", "ftp://example.com/hook", vec![]),
    )
    .await;
    assert_eq!(error_code(err.unwrap_err()), "integration.invalid");
}

// ------------------------------------------------------- test_integration

#[tokio::test]
async fn test_integration_sends_valid_feishu_payload_and_reports_failures() {
    let (state, _dir) = boot_with_memory_secrets().await;

    // Happy path: business-ok Feishu envelope.
    let (server, captured) = capturing_feishu_server().await;
    let ok_row = commands::impl_upsert_integration(
        &state,
        feishu_input("ok-bot", server.uri().as_str(), vec![]),
    )
    .await
    .unwrap();
    let result = commands::impl_test_integration(&state, ok_row.id)
        .await
        .unwrap();
    assert!(result.ok, "unexpected error: {:?}", result.error);

    {
        let bodies = captured.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        let body = &bodies[0];
        assert_eq!(body["msg_type"], "text");
        assert_eq!(
            body["content"]["text"],
            json!("nuomi test\nintegration check")
        );
    } // guard dropped before the next await

    // Failing endpoint: ok:false with a non-empty error that hides the URL.
    let bad = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .respond_with(wiremock::ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&bad)
        .await;
    let raw_bad = format!("{}/hook/a1b2c3d4e5f6x9z8", bad.uri());
    let bad_row =
        commands::impl_upsert_integration(&state, feishu_input("bad-bot", &raw_bad, vec![]))
            .await
            .unwrap();
    let result = commands::impl_test_integration(&state, bad_row.id)
        .await
        .unwrap();
    assert!(!result.ok);
    let err_text = result.error.expect("error populated on failure");
    assert!(!err_text.trim().is_empty());
    assert!(
        !err_text.contains(raw_bad.as_str()),
        "full URL leaked in error text: {err_text}"
    );

    // Unknown id surfaces as the stable IPC error.
    let miss = commands::impl_test_integration(&state, "ghost".into()).await;
    assert_eq!(error_code(miss.unwrap_err()), "integration.not_found");
}

// ---------------------------------------------------------------- delete

#[tokio::test]
async fn delete_missing_integration_reports_not_found() {
    let (state, _dir) = boot_with_memory_secrets().await;
    let raw = "https://open.feishu.cn/open-apis/bot/v2/hook/a1b2c3d4e5f6x9z8";
    let row = commands::impl_upsert_integration(&state, feishu_input("doomed", raw, vec![]))
        .await
        .unwrap();
    commands::impl_delete_integration(&state, row.id)
        .await
        .unwrap();

    let err = commands::impl_delete_integration(&state, "ghost".into()).await;
    assert_eq!(error_code(err.unwrap_err()), "integration.not_found");
    assert!(commands::impl_list_integrations(&state)
        .await
        .unwrap()
        .is_empty());
}

// ------------------------------------------------------ dispatcher smoke

/// B3 smoke: after `notifier::spawn`, one published `run.state_changed`
/// reaches the enabled whitelisted Feishu sink; the message body carries
/// the topic. Polls up to 5s for the loopback request.
#[tokio::test]
async fn dispatcher_routes_published_bus_event_to_whitelisted_sink() {
    let (state, _dir) = boot_with_memory_secrets().await;
    let (server, captured) = capturing_feishu_server().await;
    commands::impl_upsert_integration(
        &state,
        feishu_input(
            "smoke-bot",
            server.uri().as_str(),
            vec!["run.state_changed".into()],
        ),
    )
    .await
    .unwrap();

    // Subscribes synchronously before returning: publishing right after is
    // race-free even though materialization still runs in the background.
    notifier::spawn(&state);

    state.kernel.context().publish(Event::new(
        "run.state_changed",
        json!({ "runId": "r-smoke", "to": "succeeded" }),
    ));

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while captured.lock().unwrap().is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "dispatcher never delivered the event"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let bodies = captured.lock().unwrap();
    assert_eq!(bodies.len(), 1);
    let text = bodies[0]["content"]["text"].as_str().expect("feishu text");
    assert!(
        text.contains("run.state_changed"),
        "topic missing from message body: {text}"
    );
    assert!(
        text.contains("succeeded"),
        "payload missing from body: {text}"
    );
}
