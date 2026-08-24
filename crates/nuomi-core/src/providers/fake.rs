//! Deterministic fake provider for tests and demo mode.
//!
//! Plays back a scripted sequence of responses; extra calls receive the
//! last entry repeatedly.

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use std::collections::VecDeque;
use std::sync::Mutex;

use super::client::LlmProvider;
use super::types::{ChatRequest, ChatResponse, StreamEvent};
use super::ProviderError;

pub struct FakeLlm {
    tag: String,
    script: Mutex<VecDeque<ChatResponse>>,
    pub requests: Mutex<Vec<ChatRequest>>,
}

impl FakeLlm {
    pub fn new(tag: impl Into<String>, script: Vec<ChatResponse>) -> Self {
        Self {
            tag: tag.into(),
            script: Mutex::new(VecDeque::from(script)),
            requests: Mutex::new(Vec::new()),
        }
    }

    pub fn response(content: &str) -> ChatResponse {
        ChatResponse {
            content: content.to_string(),
            ..ChatResponse::default()
        }
    }

    /// Appends responses to the script (test convenience).
    pub fn extend_script(&self, extra: impl IntoIterator<Item = ChatResponse>) {
        self.script.lock().unwrap().extend(extra);
    }
}

impl FakeLlm {
    fn next(&self, request: &ChatRequest) -> ChatResponse {
        self.requests.lock().unwrap().push(request.clone());
        self.script.lock().unwrap().pop_front().unwrap_or_default()
    }
}

#[async_trait]
impl LlmProvider for FakeLlm {
    fn id(&self) -> &str {
        &self.tag
    }

    async fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        Ok(self.next(request))
    }

    fn stream(
        &self,
        request: &ChatRequest,
    ) -> BoxStream<'static, Result<StreamEvent, ProviderError>> {
        let resp = self.next(request);
        futures::stream::iter(vec![
            Ok(StreamEvent::TextDelta(resp.content.clone())),
            Ok(StreamEvent::Completed(resp)),
        ])
        .boxed()
    }
}
