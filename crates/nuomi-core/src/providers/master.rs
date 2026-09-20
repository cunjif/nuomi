//! Master-slave orchestration:
//! 1) capability routing with ordered fallback across slave providers;
//! 2) exposing slave providers as tools the master model can delegate to.

use std::sync::Arc;

use serde_json::json;

use super::client::LlmProvider;
use super::types::{ChatRequest, ChatResponse, ToolCall, ToolDef};
use super::ProviderError;

/// A slave endpoint registered with the router.
pub struct SlaveEndpoint {
    pub id: String,
    pub capabilities: Vec<String>,
    /// Lower is tried earlier within equally-matching endpoints.
    pub fallback_order: i64,
    pub provider: Arc<dyn LlmProvider>,
}

/// Routes requests from the master to capability-matched slaves.
pub struct MasterSlaveRouter {
    master: Arc<dyn LlmProvider>,
    slaves: Vec<SlaveEndpoint>,
}

impl MasterSlaveRouter {
    pub fn new(master: Arc<dyn LlmProvider>, slaves: Vec<SlaveEndpoint>) -> Self {
        let mut slaves = slaves;
        slaves.sort_by_key(|s| s.fallback_order);
        Self { master, slaves }
    }

    pub fn master(&self) -> Arc<dyn LlmProvider> {
        self.master.clone()
    }

    /// Slaves whose capability set contains **all** required tags,
    /// in deterministic (fallback_order, id) order.
    pub(crate) fn match_slaves(&self, required: &[String]) -> Vec<&SlaveEndpoint> {
        self.slaves
            .iter()
            .filter(|s| required.iter().all(|c| s.capabilities.contains(c)))
            .collect()
    }

    /// Sends `request` through every matching slave in order until one
    /// succeeds. No matches → `NoMatchingCapability`; all fail →
    /// `AllFallbacksFailed`.
    pub async fn complete_via_slave(
        &self,
        required_capabilities: &[String],
        request: &ChatRequest,
    ) -> Result<(Arc<dyn LlmProvider>, ChatResponse), ProviderError> {
        let candidates = self.match_slaves(required_capabilities);
        if candidates.is_empty() {
            return Err(ProviderError::NoMatchingCapability(
                required_capabilities.join(","),
            ));
        }
        let mut failures = Vec::new();
        for candidate in &candidates {
            match candidate.provider.complete(request).await {
                Ok(response) => return Ok((candidate.provider.clone(), response)),
                Err(e) => failures.push(format!("{}: {e}", candidate.id)),
            }
        }
        Err(ProviderError::AllFallbacksFailed(failures.join("; ")))
    }
}

/// Wraps the router so the master model can call slave providers as tools.
pub struct SlaveAsTool {
    router: Arc<MasterSlaveRouter>,
}

impl SlaveAsTool {
    pub fn new(router: Arc<MasterSlaveRouter>) -> Self {
        Self { router }
    }

    /// One tool per slave endpoint: `delegate_<sanitized-id>`.
    pub fn tool_defs(&self) -> Vec<ToolDef> {
        self.router
            .slaves
            .iter()
            .map(|s| ToolDef {
                name: sanitize_tool_name(&format!("delegate_{}", s.id)),
                description: format!(
                    "Delegate a sub-task to slave provider '{}' (capabilities: {}).",
                    s.id,
                    if s.capabilities.is_empty() {
                        "-".into()
                    } else {
                        s.capabilities.join(",")
                    }
                ),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "prompt": { "type": "string", "description": "The sub-task instruction." }
                    },
                    "required": ["prompt"]
                }),
            })
            .collect()
    }

    /// Executes a delegation tool call against the named slave.
    pub async fn execute(&self, call: &ToolCall) -> Result<String, ProviderError> {
        let endpoint = self
            .router
            .slaves
            .iter()
            .find(|s| sanitize_tool_name(&format!("delegate_{}", s.id)) == call.name)
            .ok_or_else(|| ProviderError::NotConfigured(call.name.clone()))?;
        let prompt = call
            .arguments
            .get("prompt")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let request = ChatRequest::simple("default", "You are a delegated sub-agent.", prompt);
        let response = endpoint.provider.complete(&request).await?;
        Ok(response.content)
    }
}

/// Tool names must satisfy `[a-zA-Z0-9_-]`.
fn sanitize_tool_name(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::types::{MessageRole, Usage};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct FakeProvider {
        tag: &'static str,
        fail_first: AtomicUsize,
    }

    #[async_trait]
    impl LlmProvider for FakeProvider {
        fn id(&self) -> &str {
            self.tag
        }
        async fn complete(&self, _request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
            if self.fail_first.load(Ordering::SeqCst) > 0 {
                self.fail_first.fetch_sub(1, Ordering::SeqCst);
                return Err(ProviderError::Protocol {
                    provider: "fake",
                    message: self.tag.into(),
                });
            }
            Ok(ChatResponse {
                content: format!("from:{}", self.tag),
                tool_calls: vec![],
                usage: Some(Usage {
                    prompt_tokens: 1,
                    completion_tokens: 1,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                }),
                finish_reason: Some("stop".into()),
                cli_session_id: None,
            })
        }
        fn stream(
            &self,
            _request: &ChatRequest,
        ) -> futures::stream::BoxStream<'static, Result<crate::providers::StreamEvent, ProviderError>>
        {
            unimplemented!()
        }
    }

    fn req() -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            system_prompt: None,
            messages: vec![crate::providers::ChatMessage {
                role: MessageRole::User,
                content: "hi".into(),
                tool_calls: vec![],
                tool_call_id: None,
            }],
            tools: vec![],
            temperature: None,
            max_tokens: None,
            cache_retention: Default::default(),
            cache_scope: None,
            external_session_id: None,
        }
    }

    fn router() -> Arc<MasterSlaveRouter> {
        Arc::new(MasterSlaveRouter::new(
            Arc::new(FakeProvider {
                tag: "master",
                fail_first: AtomicUsize::new(0),
            }),
            vec![
                SlaveEndpoint {
                    id: "fast".into(),
                    capabilities: vec!["code".into()],
                    fallback_order: 0,
                    provider: Arc::new(FakeProvider {
                        tag: "fast",
                        fail_first: AtomicUsize::new(u8::MAX as usize),
                    }),
                },
                SlaveEndpoint {
                    id: "slow".into(),
                    capabilities: vec!["code".into(), "long_context".into()],
                    fallback_order: 1,
                    provider: Arc::new(FakeProvider {
                        tag: "slow",
                        fail_first: AtomicUsize::new(0),
                    }),
                },
            ],
        ))
    }

    #[test]
    fn no_match_errors_when_capability_missing() {
        let r = router();
        assert!(matches!(
            futures::executor::block_on(r.complete_via_slave(&["vision".to_string()], &req())),
            Err(ProviderError::NoMatchingCapability(_))
        ));
    }

    #[test]
    fn fallback_tries_next_candidate_on_failure() {
        let r = router();
        let (_, resp) =
            futures::executor::block_on(r.complete_via_slave(&["code".to_string()], &req()))
                .unwrap();
        // "fast" fails first, "slow" answers.
        assert_eq!(resp.content, "from:slow");
    }

    #[test]
    fn all_failures_aggregated() {
        let r = Arc::new(MasterSlaveRouter::new(
            Arc::new(FakeProvider {
                tag: "m",
                fail_first: AtomicUsize::new(0),
            }),
            vec![SlaveEndpoint {
                id: "a".into(),
                capabilities: vec!["code".into()],
                fallback_order: 0,
                provider: Arc::new(FakeProvider {
                    tag: "a",
                    fail_first: AtomicUsize::new(u8::MAX as usize),
                }),
            }],
        ));
        match futures::executor::block_on(r.complete_via_slave(&["code".to_string()], &req())) {
            Err(ProviderError::AllFallbacksFailed(msg)) => assert!(msg.contains("a")),
            Err(e) => panic!("unexpected error: {e}"),
            Ok((_, resp)) => panic!("unexpected success: {}", resp.content),
        }
    }

    #[tokio::test]
    async fn slave_as_tool_roundtrip() {
        let r = router();
        let tools = SlaveAsTool::new(r);
        let defs = tools.tool_defs();
        assert_eq!(defs.len(), 2);
        let slow = defs.iter().find(|d| d.name.contains("slow")).unwrap();
        let call = ToolCall {
            id: "t1".into(),
            name: slow.name.clone(),
            arguments: serde_json::json!({ "prompt": "summarize this" }),
        };
        let out = tools.execute(&call).await.unwrap();
        assert_eq!(out, "from:slow");
    }

    #[tokio::test]
    async fn unknown_delegate_target_is_not_configured() {
        let r = router();
        let tools = SlaveAsTool::new(r);
        let call = ToolCall {
            id: "t1".into(),
            name: "delegate_ghost".into(),
            arguments: serde_json::json!({}),
        };
        assert!(matches!(
            tools.execute(&call).await,
            Err(ProviderError::NotConfigured(_))
        ));
    }
}
