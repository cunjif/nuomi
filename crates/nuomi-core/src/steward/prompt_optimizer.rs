//! Prompt optimizer (task 4.7, design §2.2.2-A4): one-shot
//! `LlmProvider.complete` rewrite of the composer draft. Fixed rewrite
//! system prompt, 30s hard timeout (spec 4.1-6). Failures never touch the
//! user's draft — the caller only receives the error.

use std::sync::Arc;
use std::time::Duration;

use crate::providers::types::{CacheRetention, ChatRequest};
use crate::providers::{ChatMessage, LlmProvider, SecretStore};
use crate::steward::kernel::resolve_steward_binding;
use crate::steward::StewardError;

/// Hard timeout for one optimize call (spec 4.1-6).
pub const OPTIMIZE_TIMEOUT: Duration = Duration::from_secs(30);

/// Fixed rewrite instruction — the optimizer's entire system prompt.
pub(crate) const REWRITE_SYSTEM_PROMPT: &str = "你是一个提示词优化助手。用户会给你一段发给应用管家的草稿提示词，请将其改写为结构更完整、意图更明确的提示词：补全必要的上下文与目标描述、明确期望的输出形式，但严禁改变用户的原始意图，严禁添加用户没有提到的要求。直接输出优化后的提示词文本，不要任何解释、前缀或引号。";

/// Neutral rewrite instruction for the session-generic optimize path
/// (ordinary RoleAgent/group conversations): same contract as
/// [`REWRITE_SYSTEM_PROMPT`] without the steward-specific wording.
pub(crate) const GENERIC_REWRITE_SYSTEM_PROMPT: &str = "你是一个提示词优化助手。用户会给你一段即将发送给 AI 助手的草稿提示词，请将其改写为结构更完整、意图更明确的提示词：补全必要的上下文与目标描述、明确期望的输出形式，但严禁改变用户的原始意图，严禁添加用户没有提到的要求。直接输出优化后的提示词文本，不要任何解释、前缀或引号。";

#[derive(Debug)]
pub enum OptimizeError {
    /// The 30s deadline elapsed (spec 5.3.3-3c).
    Timeout,
    /// Provider call failed, steward unbound, or empty reply.
    Failed(String),
    /// chat_rich_bind 5.6.1-3 (D9): no steward model bound — the frontend
    /// maps this stable code to the "bind the steward agent first" nudge.
    /// Distinct from `Failed` so the generic failure toast never masks the
    /// actionable setup guidance.
    StewardUnbound,
}

impl std::fmt::Display for OptimizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout => write!(f, "提示词优化超时，请重试"),
            Self::Failed(msg) => write!(f, "提示词优化失败：{msg}"),
            Self::StewardUnbound => write!(f, "管家未绑定模型服务，请先在设置中绑定"),
        }
    }
}

/// Optimizes a composer draft with one LLM call on the steward's bound
/// provider. Only a successful rewrite returns a value; every failure path
/// returns an error and the caller keeps the user's original draft.
pub async fn optimize_prompt(
    db_path: Arc<str>,
    secrets: Arc<dyn SecretStore>,
    session_id: &str,
    draft: &str,
) -> Result<String, OptimizeError> {
    // Session must exist — the optimizer is scoped to a steward chat.
    {
        let path = Arc::clone(&db_path);
        let sid = session_id.to_string();
        tokio::task::spawn_blocking(move || -> Result<(), StewardError> {
            let db = crate::store::Db::open(&path)?;
            if crate::store::repos::steward::get_steward_session(&db.0, &sid).is_err() {
                return Err(StewardError::NotFound {
                    entity: "steward_session",
                    id: sid,
                });
            }
            Ok(())
        })
        .await
        .map_err(|e| OptimizeError::Failed(format!("join error: {e}")))?
        .map_err(|e| OptimizeError::Failed(e.to_string()))?;
    }

    if draft.trim().is_empty() {
        return Err(OptimizeError::Failed("草稿为空".into()));
    }

    let binding = resolve_steward_binding(&db_path, secrets)
        .await
        .map_err(|e| OptimizeError::Failed(e.to_string()))?
        .ok_or_else(|| OptimizeError::Failed("管家未绑定模型服务，请先在设置中绑定".into()))?;

    optimize_with_provider(binding.provider, &binding.model, draft, OPTIMIZE_TIMEOUT).await
}

/// Provider-injectable core: fixed rewrite prompt + one `complete` call
/// wrapped in a timeout. Split out so tests can drive it with fakes.
pub(crate) async fn optimize_with_provider(
    provider: Arc<dyn LlmProvider>,
    model: &str,
    draft: &str,
    timeout: Duration,
) -> Result<String, OptimizeError> {
    optimize_with_system_prompt(provider, model, draft, REWRITE_SYSTEM_PROMPT, timeout).await
}

/// Session-generic entry for ordinary (non-steward) conversations: same
/// one-shot rewrite contract as [`optimize_with_provider`] with a neutral
/// system prompt, running on the caller-resolved provider.
pub async fn optimize_draft_with_provider(
    provider: Arc<dyn LlmProvider>,
    model: &str,
    draft: &str,
) -> Result<String, OptimizeError> {
    optimize_with_system_prompt(
        provider,
        model,
        draft,
        GENERIC_REWRITE_SYSTEM_PROMPT,
        OPTIMIZE_TIMEOUT,
    )
    .await
}

async fn optimize_with_system_prompt(
    provider: Arc<dyn LlmProvider>,
    model: &str,
    draft: &str,
    system_prompt: &str,
    timeout: Duration,
) -> Result<String, OptimizeError> {
    let request = ChatRequest {
        model: model.to_string(),
        system_prompt: Some(system_prompt.to_string()),
        messages: vec![ChatMessage::user(draft.to_string())],
        tools: Vec::new(),
        temperature: Some(0.3),
        max_tokens: None,
        cache_retention: CacheRetention::None,
        cache_scope: None,
        external_session_id: None,
    };

    let optimized = match tokio::time::timeout(timeout, provider.complete(&request)).await {
        Err(_elapsed) => return Err(OptimizeError::Timeout),
        Ok(Err(e)) => return Err(OptimizeError::Failed(e.to_string())),
        Ok(Ok(response)) => response.content.trim().to_string(),
    };
    if optimized.is_empty() {
        return Err(OptimizeError::Failed("模型返回了空结果".into()));
    }
    Ok(optimized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::fake::FakeLlm;
    use crate::providers::types::{ChatResponse, StreamEvent};
    use crate::providers::{MemorySecretStore, ProviderError};
    use crate::store::migrations;
    use futures::StreamExt;

    async fn db_path() -> Arc<str> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opt.db");
        let path_str = path.to_string_lossy().to_string();
        let db = crate::store::Db::open(&path_str).unwrap();
        migrations::run(&db.0).unwrap();
        drop(db);
        std::mem::forget(dir);
        Arc::from(path_str)
    }

    fn seed_session(conn: &rusqlite::Connection, id: &str) {
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at, kind)
             VALUES (?1, 'test', 1, 1, 'background')",
            [id],
        )
        .unwrap();
        crate::store::repos::steward::insert_steward_session(
            conn,
            &crate::domain::StewardSession {
                id: id.into(),
                steward_id: "steward".into(),
                title: "test".into(),
                goal: None,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
    }

    async fn setup() -> (Arc<str>, Arc<dyn SecretStore>) {
        let db = db_path().await;
        {
            let conn = crate::store::Db::open(&db).unwrap();
            seed_session(&conn.0, "s1");
        }
        (db, Arc::new(MemorySecretStore::default()))
    }

    #[tokio::test]
    async fn unknown_session_fails() {
        let (db, secrets) = setup().await;
        let err = optimize_prompt(db, secrets, "nope", "帮我看看")
            .await
            .unwrap_err();
        assert!(matches!(err, OptimizeError::Failed(_)));
    }

    #[tokio::test]
    async fn empty_draft_fails_without_provider_call() {
        let (db, secrets) = setup().await;
        let err = optimize_prompt(db, secrets, "s1", "   ").await.unwrap_err();
        assert!(matches!(err, OptimizeError::Failed(_)));
    }

    #[tokio::test]
    async fn unbound_role_fails_with_binding_hint() {
        let (db, secrets) = setup().await;
        let err = optimize_prompt(db, secrets, "s1", "帮我诊断")
            .await
            .unwrap_err();
        match err {
            OptimizeError::Failed(msg) => assert!(msg.contains("绑定")),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn rewrite_uses_fixed_prompt_and_trims_reply() {
        let fake = Arc::new(FakeLlm::new(
            "fake",
            vec![FakeLlm::response("  优化后的提示词  ")],
        ));
        let provider: Arc<dyn LlmProvider> = Arc::clone(&fake) as _;
        let out = optimize_with_provider(provider, "m", "草稿", Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(out, "优化后的提示词");

        let req = &fake.requests.lock().unwrap()[0];
        assert_eq!(req.system_prompt.as_deref(), Some(REWRITE_SYSTEM_PROMPT));
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].content, "草稿");
        assert!(req.tools.is_empty());
    }

    #[tokio::test]
    async fn empty_reply_maps_to_failed() {
        // Empty script → FakeLlm plays back the default (empty) response.
        let fake = FakeLlm::new("fake", vec![]);
        let provider: Arc<dyn LlmProvider> = Arc::new(fake);
        let err = optimize_with_provider(provider, "m", "草稿", Duration::from_secs(5))
            .await
            .unwrap_err();
        match err {
            OptimizeError::Failed(msg) => assert!(msg.contains("空结果")),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// A provider whose `complete` never resolves — drives the timeout arm.
    struct HangingProvider;

    #[async_trait::async_trait]
    impl LlmProvider for HangingProvider {
        fn id(&self) -> &str {
            "hang"
        }
        async fn complete(&self, _request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
            std::future::pending::<()>().await;
            unreachable!()
        }
        fn stream(
            &self,
            _request: &ChatRequest,
        ) -> futures::stream::BoxStream<'static, Result<StreamEvent, ProviderError>> {
            futures::stream::pending().boxed()
        }
    }

    #[tokio::test]
    async fn deadline_elapsed_maps_to_timeout() {
        let provider: Arc<dyn LlmProvider> = Arc::new(HangingProvider);
        let err = optimize_with_provider(provider, "m", "草稿", Duration::from_millis(50))
            .await
            .unwrap_err();
        assert!(matches!(err, OptimizeError::Timeout));
        assert_eq!(err.to_string(), "提示词优化超时，请重试");
    }

    /// A provider that always errors — drives the failure arm.
    struct FailingProvider;

    #[async_trait::async_trait]
    impl LlmProvider for FailingProvider {
        fn id(&self) -> &str {
            "fail"
        }
        async fn complete(&self, _request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
            Err(ProviderError::NotConfigured("fail".into()))
        }
        fn stream(
            &self,
            _request: &ChatRequest,
        ) -> futures::stream::BoxStream<'static, Result<StreamEvent, ProviderError>> {
            futures::stream::iter(vec![Err(ProviderError::NotConfigured("fail".into()))]).boxed()
        }
    }

    #[tokio::test]
    async fn provider_error_maps_to_failed() {
        let provider: Arc<dyn LlmProvider> = Arc::new(FailingProvider);
        let err = optimize_with_provider(provider, "m", "草稿", Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(matches!(err, OptimizeError::Failed(_)));
    }

    #[test]
    fn timeout_constant_is_30s() {
        assert_eq!(OPTIMIZE_TIMEOUT, Duration::from_secs(30));
    }
}
