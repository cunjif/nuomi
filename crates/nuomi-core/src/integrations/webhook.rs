//! Generic webhook outbound sink (QQ bot webhooks, telemetry-style receivers
//! and any custom HTTP endpoint): `POST {"source":"nuomi","title","body","meta":{}}`
//! with optional extra headers from the integration config.

use std::collections::BTreeMap;

use async_trait::async_trait;
use serde_json::json;

use super::{map_send_err, truncated, OutboundError, OutboundSink};
use crate::domain::IntegrationKind;

pub struct GenericWebhookSink {
    /// Reported via [`OutboundSink::kind`]; both `qq_webhook` and `telemetry`
    /// rows share this sink implementation.
    kind: IntegrationKind,
    url: String,
    headers: BTreeMap<String, String>,
    client: reqwest::Client,
}

impl GenericWebhookSink {
    pub fn new(
        kind: IntegrationKind,
        url: impl Into<String>,
        headers: BTreeMap<String, String>,
    ) -> Self {
        Self {
            kind,
            url: url.into(),
            headers,
            // Shared process-wide pool; the 10s budget from
            // `integrations::HTTP_TIMEOUT` is applied per request below.
            client: crate::providers::pool::shared_client(),
        }
    }
}

#[async_trait]
impl OutboundSink for GenericWebhookSink {
    fn kind(&self) -> IntegrationKind {
        self.kind
    }

    async fn send(&self, title: &str, body: &str) -> Result<(), OutboundError> {
        let payload = json!({
            "source": "nuomi",
            "title": title,
            "body": body,
            "meta": {},
        });
        let mut req = self
            .client
            .post(&self.url)
            .timeout(super::HTTP_TIMEOUT)
            .json(&payload);
        for (k, v) in &self.headers {
            req = req.header(k, v);
        }
        let resp = req.send().await.map_err(map_send_err)?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(OutboundError::Http(format!(
                "status {status}: {}",
                truncated(&text, 200)
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::sync::{Arc, Mutex};
    use wiremock::matchers::{header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    type Captured = Arc<Mutex<Vec<(reqwest::header::HeaderMap, Value)>>>;

    async fn ok_server() -> (MockServer, Captured) {
        let server = MockServer::start().await;
        let captured: Captured = Arc::new(Mutex::new(Vec::new()));
        let sink = captured.clone();
        Mock::given(method("POST"))
            .respond_with(move |req: &wiremock::Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                sink.lock().unwrap().push((req.headers.clone(), body));
                ResponseTemplate::new(200)
            })
            .mount(&server)
            .await;
        (server, captured)
    }

    #[tokio::test]
    async fn payload_shape_and_header_injection() {
        let (server, captured) = ok_server().await;
        let mut headers = BTreeMap::new();
        headers.insert("X-Token".to_string(), "secret-token".to_string());
        let sink = GenericWebhookSink::new(IntegrationKind::QqWebhook, server.uri(), headers);
        sink.send("deploy", "done").await.unwrap();

        let entries = captured.lock().unwrap();
        assert_eq!(entries.len(), 1);
        let (hdrs, body) = &entries[0];
        assert_eq!(body["source"], "nuomi");
        assert_eq!(body["title"], "deploy");
        assert_eq!(body["body"], "done");
        assert_eq!(body["meta"], json!({}));
        assert_eq!(
            hdrs.get("x-token").and_then(|v| v.to_str().ok()),
            Some("secret-token")
        );
    }

    #[tokio::test]
    async fn non_2xx_is_an_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503).set_body_string("unavailable"))
            .mount(&server)
            .await;
        let sink =
            GenericWebhookSink::new(IntegrationKind::QqWebhook, server.uri(), BTreeMap::new());
        let err = sink.send("t", "b").await.unwrap_err();
        assert!(matches!(err, OutboundError::Http(ref m) if m.contains("503")));
    }

    #[tokio::test]
    async fn configured_headers_reach_the_wire() {
        // matcher-level proof that the header value is injected verbatim.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(header("x-nuomi-team", "core"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let mut headers = BTreeMap::new();
        headers.insert("X-Nuomi-Team".to_string(), "core".to_string());
        let sink = GenericWebhookSink::new(IntegrationKind::QqWebhook, server.uri(), headers);
        sink.send("t", "b").await.unwrap();
        server.verify().await;
    }
}
