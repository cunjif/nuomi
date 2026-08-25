//! Outbound integrations (SPEC bots-telemetry-m1): bot webhooks and the
//! telemetry exporter, materialized from the `integrations` table.
//!
//! Every delivery endpoint implements [`OutboundSink`]; [`materialize`] loads
//! enabled rows from SQLite and builds one sink per row. HTTP failures are
//! reported, never panicked; telemetry batches are dropped on error without
//! backlog.

pub mod feishu;
pub mod telemetry;
pub mod webhook;

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use async_trait::async_trait;
use base64::Engine as _;
use thiserror::Error;

use crate::domain::{Integration, IntegrationKind};
use crate::store::{migrations, repos, Db};

/// Shared HTTP timeout for all outbound integrations.
pub(crate) const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Response-body excerpt length kept inside [`OutboundError::Http`].
const ERROR_BODY_MAX_CHARS: usize = 200;

#[derive(Debug, Error)]
pub enum OutboundError {
    #[error("http error: {0}")]
    Http(String),
    #[error("config error: {0}")]
    Config(String),
    #[error("request timed out")]
    Timeout,
}

/// One outbound delivery endpoint (bot webhook or telemetry receiver).
#[async_trait]
pub trait OutboundSink: Send + Sync {
    fn kind(&self) -> IntegrationKind;
    async fn send(&self, title: &str, body: &str) -> Result<(), OutboundError>;
}

// ---------------------------------------------------------------- http glue

static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

/// Crate-level shared HTTP client (10s timeout); falls back to a default
/// client in the pathological case that TLS init fails (never panics).
pub(crate) fn http_client() -> reqwest::Client {
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(HTTP_TIMEOUT)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new())
        })
        .clone()
}

pub(crate) fn map_send_err(e: reqwest::Error) -> OutboundError {
    if e.is_timeout() {
        OutboundError::Timeout
    } else {
        OutboundError::Http(e.to_string())
    }
}

/// Char-safe truncation for error excerpts.
pub(crate) fn truncated(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        s.chars().take(max_chars).collect()
    }
}

/// POSTs JSON and maps non-2xx responses to [`OutboundError::Http`] with a
/// truncated body excerpt.
pub(crate) async fn post_json_checked(
    client: &reqwest::Client,
    url: &str,
    payload: &serde_json::Value,
) -> Result<String, OutboundError> {
    let resp = client
        .post(url)
        .json(payload)
        .send()
        .await
        .map_err(map_send_err)?;
    let status = resp.status();
    let text = resp.text().await.map_err(map_send_err)?;
    if !status.is_success() {
        return Err(OutboundError::Http(format!(
            "status {status}: {}",
            truncated(&text, ERROR_BODY_MAX_CHARS)
        )));
    }
    Ok(text)
}

/// BASE64(HMAC_SHA256(key = "{ts}\n{secret}", message = "")) — the Feishu
/// custom-bot signing algorithm.
pub(crate) fn feishu_sign(ts_seconds: i64, secret: &str) -> Result<String, OutboundError> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let key = format!("{ts_seconds}\n{secret}");
    let mut mac = HmacSha256::new_from_slice(key.as_bytes())
        .map_err(|e| OutboundError::Config(format!("hmac key rejected: {e}")))?;
    mac.update(b"");
    Ok(base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes()))
}

// ------------------------------------------------------------ materialize

/// Loads integrations from SQLite on the blocking pool and builds one sink
/// per **enabled** row. Rows whose config lacks a usable `webhook_url` are
/// skipped and reported in the returned warnings list.
///
/// Config keys: `webhook_url` (required), `secret` (feishu only, optional),
/// `headers` (generic webhook/telemetry, optional string map).
pub async fn materialize(
    db_path: impl Into<Arc<str>>,
) -> Result<(Vec<(Integration, Arc<dyn OutboundSink>)>, Vec<String>), crate::store::StoreError> {
    let path = db_path.into();
    let rows = tokio::task::spawn_blocking(
        move || -> Result<Vec<Integration>, crate::store::StoreError> {
            let db = Db::open(&path)?;
            migrations::run(&db.0)?;
            repos::integrations::list(&db.0)
        },
    )
    .await
    .map_err(|e| {
        crate::store::StoreError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
    })??;

    let mut sinks = Vec::new();
    let mut warnings = Vec::new();
    for integration in rows {
        if !integration.enabled {
            continue;
        }
        match build_sink(&integration) {
            Ok(sink) => sinks.push((integration, sink)),
            Err(msg) => warnings.push(msg),
        }
    }
    Ok((sinks, warnings))
}

/// Dispatches one integration row to its sink implementation.
fn build_sink(integration: &Integration) -> Result<Arc<dyn OutboundSink>, String> {
    fn field(config: &serde_json::Value, key: &str) -> Option<String> {
        config.get(key).and_then(|v| v.as_str()).map(str::to_string)
    }

    let url = field(&integration.config, "webhook_url").ok_or_else(|| {
        format!(
            "integration '{}' ({}) skipped: missing 'webhook_url' in config",
            integration.name,
            integration.kind.as_str()
        )
    })?;

    Ok(match integration.kind {
        IntegrationKind::FeishuBot => Arc::new(feishu::FeishuSink::new(
            url,
            field(&integration.config, "secret"),
        )),
        IntegrationKind::QqWebhook | IntegrationKind::Telemetry => {
            let mut headers = std::collections::BTreeMap::new();
            if let Some(obj) = integration
                .config
                .get("headers")
                .and_then(|v| v.as_object())
            {
                for (k, v) in obj {
                    if let Some(s) = v.as_str() {
                        headers.insert(k.clone(), s.to_string());
                    }
                }
            }
            Arc::new(webhook::GenericWebhookSink::new(
                integration.kind,
                url,
                headers,
            ))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{new_id, IntegrationKind};
    use crate::store::migrations;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn row(
        name: &str,
        kind: IntegrationKind,
        config: serde_json::Value,
        enabled: bool,
    ) -> Integration {
        Integration {
            id: new_id(),
            name: name.into(),
            kind,
            config,
            events: vec![],
            enabled,
            created_at: 1,
            updated_at: 1,
        }
    }

    /// Same as [`row`] but with an explicit `created_at` for deterministic
    /// list ordering.
    fn row_at(
        name: &str,
        kind: IntegrationKind,
        config: serde_json::Value,
        enabled: bool,
        created_at: i64,
    ) -> Integration {
        let mut r = row(name, kind, config, enabled);
        r.created_at = created_at;
        r
    }

    async fn seeded_db(rows: &[Integration]) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("int.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            migrations::run(&conn).unwrap();
            for r in rows {
                repos::integrations::insert(&conn, r).unwrap();
            }
        }
        (dir, path.to_string_lossy().into_owned())
    }

    #[tokio::test]
    async fn materializes_only_enabled_rows_with_urls_and_reports_warnings() {
        // disabled integration points at its own server: must never be hit
        let disabled_server = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&disabled_server)
            .await;

        let rows = [
            row(
                "feishu",
                IntegrationKind::FeishuBot,
                serde_json::json!({ "webhook_url": "https://feishu.test/hook", "secret": "s" }),
                true,
            ),
            row(
                "no-url",
                IntegrationKind::QqWebhook,
                serde_json::json!({}),
                true,
            ),
            row(
                "off-telemetry",
                IntegrationKind::Telemetry,
                serde_json::json!({ "webhook_url": disabled_server.uri() }),
                false,
            ),
        ];
        let (_dir, path) = seeded_db(&rows).await;

        let (sinks, warnings) = materialize(path).await.unwrap();
        assert_eq!(sinks.len(), 1, "only the enabled feishu row materializes");
        assert_eq!(sinks[0].0.name, "feishu");
        assert_eq!(sinks[0].1.kind(), IntegrationKind::FeishuBot);
        assert_eq!(
            warnings.len(),
            1,
            "missing webhook_url yields exactly one warning"
        );
        assert!(warnings[0].contains("no-url") && warnings[0].contains("qq_webhook"));
        // the disabled row never issued a request
        disabled_server.verify().await;
    }

    #[tokio::test]
    async fn kind_dispatch_matches_row_kind() {
        let rows = [
            row_at(
                "qq",
                IntegrationKind::QqWebhook,
                serde_json::json!({ "webhook_url": "https://qq.test/hook" }),
                true,
                10,
            ),
            row_at(
                "tel",
                IntegrationKind::Telemetry,
                serde_json::json!({ "webhook_url": "https://tel.test/ndjson" }),
                true,
                20,
            ),
        ];
        let (_dir, path) = seeded_db(&rows).await;

        let (sinks, warnings) = materialize(path).await.unwrap();
        assert!(warnings.is_empty());
        assert_eq!(sinks.len(), 2);
        assert_eq!(sinks[0].1.kind(), IntegrationKind::QqWebhook);
        assert_eq!(sinks[1].1.kind(), IntegrationKind::Telemetry);
    }

    #[tokio::test]
    async fn missing_db_reports_store_error() {
        let dir = tempfile::tempdir().unwrap();
        let bad = dir
            .path()
            .join("no-such-dir")
            .join("x")
            .to_string_lossy()
            .into_owned();
        assert!(materialize(bad).await.is_err());
    }

    #[test]
    fn feishu_sign_rejects_nothing_but_stays_total() {
        // sanity: signing works for arbitrary ts/secret lengths
        assert!(feishu_sign(0, "").is_ok());
        assert!(feishu_sign(i64::MAX, "long-secret-".repeat(64).as_str()).is_ok());
    }
}
