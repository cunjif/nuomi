//! Model catalog fetch for the settings UI (KiloCode-style "pull the model
//! list after entering base URL + key").
//!
//! One request — `GET {base_url}/models` — for both supported protocols,
//! differing only in the auth headers:
//! - OpenAI-compatible: `Authorization: Bearer <key>`
//! - Anthropic-compatible: `x-api-key: <key>` + `anthropic-version`
//!
//! Only ids come back: capability tags (Re/I/Vo/Vi) stay a user decision, so
//! the caller decides what to do with the ids (the settings form seeds
//! `reasoning` and lets the badges refine it afterwards).

use super::ProviderError;
use crate::domain::ProviderProtocol;
use serde::Deserialize;

/// Fetch budget: a wedged endpoint must fail fast inside the modal form.
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

/// Protocol tag used in [`ProviderError::Protocol`] messages.
const CATALOG: &str = "models";

#[derive(Debug, Deserialize)]
struct ModelsEnvelope {
    /// Missing/empty payload degrades to "no models" instead of an error.
    #[serde(default)]
    data: Option<Vec<ModelEntry>>,
}

/// Endpoints are inconsistent: OpenAI/Anthropic send `{"id": "…"}`, a few
/// proxies send bare strings. Both shapes are accepted.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ModelEntry {
    Named { id: String },
    Plain(String),
}

impl ModelEntry {
    fn id(self) -> String {
        match self {
            ModelEntry::Named { id } => id,
            ModelEntry::Plain(id) => id,
        }
    }
}

/// Sorted, de-duplicated model ids exposed by the endpoint. `proxy` routes
/// the request through a per-provider local proxy (see
/// [`super::pool::client_for_endpoint`]); `None` = direct connection.
pub async fn list_model_ids(
    protocol: ProviderProtocol,
    base_url: &str,
    api_key: &str,
    proxy: Option<&str>,
) -> Result<Vec<String>, ProviderError> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err(ProviderError::Protocol {
            provider: CATALOG,
            message: "base url is empty".to_string(),
        });
    }
    let url = format!("{base}/models");
    let http = super::pool::client_for_endpoint(proxy)?;
    let mut request = http.get(&url).timeout(FETCH_TIMEOUT);
    request = match protocol {
        ProviderProtocol::OpenAiCompatible => {
            if !api_key.is_empty() {
                request = request.bearer_auth(api_key);
            }
            request
        }
        ProviderProtocol::AnthropicCompatible => {
            request = request.header("anthropic-version", "2023-06-01");
            if !api_key.is_empty() {
                request = request.header("x-api-key", api_key);
            }
            request
        }
    };

    let response = request.send().await?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        let excerpt: String = body.chars().take(200).collect();
        return Err(ProviderError::Protocol {
            provider: CATALOG,
            message: format!("{status}: {excerpt}"),
        });
    }
    let envelope: ModelsEnvelope = response.json().await.map_err(|e| ProviderError::Protocol {
        provider: CATALOG,
        message: format!("unreadable payload: {e}"),
    })?;

    let mut ids: Vec<String> = envelope
        .data
        .unwrap_or_default()
        .into_iter()
        .map(ModelEntry::id)
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .collect();
    ids.sort();
    ids.dedup();
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const PAYLOAD: &str = r#"{"object":"list","data":[
        {"id":"gpt-4o","object":"model"},
        {"id":"gpt-4o-mini","object":"model"},
        {"id":"gpt-4o"}
    ]}"#;

    #[tokio::test]
    async fn parses_sorted_unique_ids_and_sends_the_bearer_key() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(PAYLOAD, "application/json"))
            .mount(&server)
            .await;

        let ids = list_model_ids(
            ProviderProtocol::OpenAiCompatible,
            &format!("{}/", server.uri()),
            "secret",
            None,
        )
        .await
        .expect("catalog fetch must succeed");
        assert_eq!(ids, vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()]);
    }

    #[tokio::test]
    async fn anthropic_uses_its_own_auth_headers() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .and(header("x-api-key", "sk-ant"))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(PAYLOAD, "application/json"))
            .mount(&server)
            .await;

        let ids = list_model_ids(
            ProviderProtocol::AnthropicCompatible,
            &server.uri(),
            "sk-ant",
            None,
        )
        .await
        .expect("anthropic catalog fetch must succeed");
        assert_eq!(ids.len(), 2);
    }

    #[tokio::test]
    async fn http_failure_surfaces_the_status() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/models"))
            .respond_with(ResponseTemplate::new(401).set_body_string("invalid api key"))
            .mount(&server)
            .await;

        let err = list_model_ids(
            ProviderProtocol::OpenAiCompatible,
            &server.uri(),
            "bad",
            None,
        )
        .await
        .expect_err("401 must be an error");
        assert!(err.to_string().contains("401"), "{err}");
    }

    #[tokio::test]
    async fn empty_base_url_is_rejected_without_a_request() {
        let err = list_model_ids(ProviderProtocol::OpenAiCompatible, "  ", "k", None)
            .await
            .expect_err("empty base url must be rejected");
        assert!(err.to_string().contains("empty"), "{err}");
    }
}
