//! Narrow adapter port and optional capability traits (hexagonal pattern,
//! adapted from agent-orchestrator §2.4, see docs/research/agent-orchestrator.md).
//!
//! The port [`AgentAdapter`] keeps only the required core every adapter must
//! provide (three methods today). Everything beyond that core lives behind
//! optional capability traits: adapters implement what they support, and
//! consumers probe via [`AdapterCapabilities`], degrading gracefully when a
//! capability is absent. Adding a new adapter means implementing the port
//! plus only the capability traits it actually carries.

use crate::providers::client::LlmProvider;

/// The narrow port every agent adapter must satisfy.
///
/// It is a pure supertrait of [`LlmProvider`] with a blanket impl, so every
/// provider is automatically an [`AgentAdapter`]: existing call sites keep
/// naming [`LlmProvider`] with zero changes, while new code (and the
/// "orchestrator depends on trait `AgentAdapter`" boundary in AGENTS.md) can
/// name the port explicitly.
///
/// Required core (startup/teardown is owned by each adapter's internal
/// stream state machine, so it is not part of the port):
/// - [`LlmProvider::id`] — stable identity;
/// - [`LlmProvider::complete`] — one-shot prompt delivery;
/// - [`LlmProvider::stream`] — streaming prompt delivery.
pub trait AgentAdapter: LlmProvider {}

impl<T: LlmProvider> AgentAdapter for T {}

/// Optional capability: resolving an executable binary against the
/// adapter's allowlist. Present on adapters that spawn child processes.
pub trait BinaryResolver: Send + Sync {
    /// Returns the canonical (lowercased basename) form of `command` when
    /// the adapter would accept it at spawn time, `None` otherwise.
    fn resolve_binary(&self, command: &str) -> Option<String>;
}

/// Optional capability: producing a sanitized child-process environment.
/// Present on process-spawning adapters only; HTTP providers never touch
/// environment variables.
pub trait EnvSanitizer: Send + Sync {
    /// Overlays `profile_env` onto `parent`, then strips blocked variables
    /// so the result is safe to install in a child process.
    fn sanitize_env(
        &self,
        parent: Vec<(String, String)>,
        profile_env: &[(String, String)],
    ) -> Vec<(String, String)>;
}

/// Optional capability: resuming a prior session through an agent-side
/// conversation handle (restore / session info). Reserved for adapter-level
/// session resume (harness-research-synthesis P1).
///
/// Deliberately an *adapter*-level capability: the interactive
/// [`crate::adapters::PtySession`] handle is a per-task object (its output
/// channel receiver is `!Sync` by design), so it carries session continuity
/// through its own API (`next_output` / `send_prompt` / `kill`) instead of
/// this trait. A future adapter wrapper that can detach and re-attach
/// `PtySession`s will implement this capability; keeping the empty marker
/// now preserves the probe surface for consumers without breaking them.
pub trait ResumableSession: Send + Sync {}

/// Capability query surface over the port.
///
/// Deliberately *not* blanket-implemented: a blanket
/// `impl<T: AgentAdapter> AdapterCapabilities for T` would collide with
/// per-adapter overrides (coherence), forcing every capability query to
/// answer `None` forever. Instead each adapter carrying capabilities writes
/// a small explicit impl — the Rust equivalent of agent-orchestrator's Go
/// type assertion `agent.(BinaryResolver)` — and adapters without extra
/// capabilities simply do not implement this trait.
pub trait AdapterCapabilities: AgentAdapter {
    /// Downcast-style access to the binary-resolution capability.
    fn as_binary_resolver(&self) -> Option<&dyn BinaryResolver> {
        None
    }

    /// Downcast-style access to the env-sanitization capability.
    fn as_env_sanitizer(&self) -> Option<&dyn EnvSanitizer> {
        None
    }

    /// Downcast-style access to the session-resume capability.
    fn as_resumable_session(&self) -> Option<&dyn ResumableSession> {
        None
    }

    /// Whether the adapter resolves binaries against an allowlist.
    fn supports_binary_resolution(&self) -> bool {
        self.as_binary_resolver().is_some()
    }

    /// Whether the adapter sanitizes child-process environments.
    fn supports_env_sanitization(&self) -> bool {
        self.as_env_sanitizer().is_some()
    }

    /// Whether the adapter can resume a prior session.
    fn supports_session_resume(&self) -> bool {
        self.as_resumable_session().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::types::{ChatRequest, ChatResponse, StreamEvent};
    use crate::providers::ProviderError;
    use async_trait::async_trait;
    use futures::stream::BoxStream;
    use futures::StreamExt;

    struct StubProvider;

    #[async_trait]
    impl LlmProvider for StubProvider {
        fn id(&self) -> &str {
            "stub"
        }

        async fn complete(&self, _request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
            Err(ProviderError::Protocol {
                provider: "stub",
                message: "not implemented".into(),
            })
        }

        fn stream(
            &self,
            _request: &ChatRequest,
        ) -> BoxStream<'static, Result<StreamEvent, ProviderError>> {
            futures::stream::once(async {
                Err(ProviderError::Protocol {
                    provider: "stub",
                    message: "not implemented".into(),
                })
            })
            .boxed()
        }
    }

    fn assert_is_agent_adapter<P: AgentAdapter>(_provider: &P) {}

    #[test]
    fn blanket_impl_makes_every_llm_provider_an_agent_adapter() {
        let stub = StubProvider;
        assert_is_agent_adapter(&stub);
        let port: &dyn AgentAdapter = &stub;
        assert_eq!(port.id(), "stub");
    }

    #[test]
    fn port_object_stream_dispatches_unchanged() {
        let stub = StubProvider;
        let port: &dyn AgentAdapter = &stub;
        let request = ChatRequest {
            model: "m".into(),
            system_prompt: None,
            messages: vec![],
            tools: Vec::new(),
            temperature: None,
            max_tokens: None,
            cache_retention: Default::default(),
            cache_scope: None,
        };
        let mut stream = port.stream(&request);
        let first = futures::executor::block_on(stream.next());
        assert!(matches!(first, Some(Err(_))));
    }
}
