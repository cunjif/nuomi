//! Feishu custom-bot outbound sink.
//!
//! Wire protocol: `POST {"msg_type":"text","content":{"text":"{title}\n{body}"}}`.
//! With a configured secret, top-level `"timestamp"` (unix seconds) and
//! `"sign"` = BASE64(HMAC_SHA256(key="{ts}\n{secret}", message="")) are added
//! (Feishu custom-bot signing algorithm). A response is accepted only when
//! HTTP status is 2xx **and** the JSON body carries `code == 0`.

use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::json;

use super::{feishu_sign as sign, post_json_checked, OutboundError, OutboundSink};
use crate::domain::IntegrationKind;

pub struct FeishuSink {
    url: String,
    secret: Option<String>,
    client: reqwest::Client,
}

impl FeishuSink {
    pub fn new(url: impl Into<String>, secret: Option<String>) -> Self {
        Self {
            url: url.into(),
            secret,
            // Shared process-wide pool; the 10s budget from
            // `integrations::HTTP_TIMEOUT` wraps the request below.
            client: crate::providers::pool::shared_client(),
        }
    }
}

fn unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        // pre-1970 clocks are not a supported runtime environment
        .unwrap_or(0)
}

#[async_trait]
impl OutboundSink for FeishuSink {
    fn kind(&self) -> IntegrationKind {
        IntegrationKind::FeishuBot
    }

    async fn send(&self, title: &str, body: &str) -> Result<(), OutboundError> {
        let mut payload = json!({
            "msg_type": "text",
            "content": { "text": format!("{title}\n{body}") },
        });
        if let Some(secret) = &self.secret {
            let ts = unix_seconds();
            payload["timestamp"] = json!(ts);
            payload["sign"] = json!(sign(ts, secret)?);
        }
        // Per-request timeout preserves the previous client-level 10s
        // semantics now that the shared (timeout-free) pool is used.
        let text = tokio::time::timeout(
            super::HTTP_TIMEOUT,
            post_json_checked(&self.client, &self.url, &payload),
        )
        .await
        .map_err(|_| OutboundError::Timeout)??;
        // Feishu reports business errors with HTTP 200 + non-zero `code`.
        let code = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("code").and_then(|c| c.as_i64()));
        match code {
            Some(c) if c != 0 => Err(OutboundError::Http(format!("feishu code {c}: {text}"))),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    use std::sync::{Arc, Mutex};
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    type Captured = Arc<Mutex<Vec<serde_json::Value>>>;

    async fn ok_server() -> (MockServer, Captured) {
        let server = MockServer::start().await;
        let captured: Captured = Arc::new(Mutex::new(Vec::new()));
        let sink = captured.clone();
        Mock::given(method("POST"))
            .respond_with(move |req: &wiremock::Request| {
                let body: serde_json::Value =
                    serde_json::from_slice(&req.body).unwrap_or(serde_json::Value::Null);
                sink.lock().unwrap().push(body);
                ResponseTemplate::new(200).set_body_json(json!({ "code": 0 }))
            })
            .mount(&server)
            .await;
        (server, captured)
    }

    #[test]
    fn sign_matches_direct_hmac_sha256_base64_computation() {
        // Independent recomputation of the documented algorithm:
        // key = "{ts}\n{secret}", empty message, standard base64.
        let ts = 1_700_000_000i64;
        let secret = "test-secret";
        let mut mac = Hmac::<Sha256>::new_from_slice(format!("{ts}\n{secret}").as_bytes()).unwrap();
        mac.update(b"");
        let expected =
            base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes());
        assert_eq!(sign(ts, secret).unwrap(), expected);
    }

    #[test]
    fn sign_matches_pinned_reference_vector() {
        // Pinned via an external HMAC-SHA256 reference implementation.
        assert_eq!(
            sign(1_700_000_000, "test-secret").unwrap(),
            "mbm4Y4oluIPQ00qlBIhX8vAZ0EKv3nw0LuTb91jPL84="
        );
    }

    #[tokio::test]
    async fn unsigned_payload_has_exact_shape() {
        let (server, captured) = ok_server().await;
        let sink = FeishuSink::new(server.uri(), None);
        sink.send("hello", "world").await.unwrap();

        let bodies = captured.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        let b = &bodies[0];
        assert_eq!(b["msg_type"], "text");
        assert_eq!(b["content"]["text"], "hello\nworld");
        assert!(b.get("timestamp").is_none(), "no timestamp without secret");
        assert!(b.get("sign").is_none(), "no sign without secret");
    }

    #[tokio::test]
    async fn signed_payload_carries_timestamp_and_sign() {
        let (server, captured) = ok_server().await;
        let sink = FeishuSink::new(server.uri(), Some("shhh".into()));
        sink.send("title", "body").await.unwrap();

        let bodies = captured.lock().unwrap();
        assert_eq!(bodies.len(), 1);
        let b = &bodies[0];
        assert!(b["timestamp"].is_i64(), "timestamp present");
        assert!(
            b["sign"].as_str().is_some_and(|s| !s.is_empty()),
            "sign present"
        );
        assert_eq!(b["content"]["text"], "title\nbody");
    }

    #[tokio::test]
    async fn nonzero_business_code_is_an_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "code": 19021, "msg": "sign error" })),
            )
            .mount(&server)
            .await;
        let sink = FeishuSink::new(server.uri(), Some("k".into()));
        let err = sink.send("t", "b").await.unwrap_err();
        assert!(matches!(err, OutboundError::Http(ref m) if m.contains("19021")));
    }

    #[tokio::test]
    async fn http_error_status_is_reported_with_truncated_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
            .mount(&server)
            .await;
        let sink = FeishuSink::new(server.uri(), None);
        let err = sink.send("t", "b").await.unwrap_err();
        assert!(
            matches!(err, OutboundError::Http(ref m) if m.contains("500") && m.contains("boom"))
        );
    }
}
