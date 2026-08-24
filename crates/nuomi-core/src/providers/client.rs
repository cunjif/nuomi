//! The provider abstraction used by the kernel and orchestrator.

use async_trait::async_trait;
use futures::stream::BoxStream;

use super::types::{ChatRequest, ChatResponse, StreamEvent};
use super::ProviderError;

/// A configured, ready-to-call LLM endpoint (protocol already bound).
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Stable identifier (usually the ProviderConfig id).
    fn id(&self) -> &str;

    /// One-shot completion.
    async fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError>;

    /// Streaming completion; the stream always ends with
    /// `StreamEvent::Completed` on success.
    fn stream(
        &self,
        request: &ChatRequest,
    ) -> BoxStream<'static, Result<StreamEvent, ProviderError>>;
}
