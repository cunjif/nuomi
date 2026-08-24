//! SystemPrompt plugin: resolves the active prompt (versioned by evolution)
//! or falls back to a static default.

use std::sync::Arc;

use crate::harness::{Context, HarnessError, Plugin};
use crate::store::repos;
use crate::store::{Db, StoreError};
use async_trait::async_trait;

/// Registered as `"system_prompt"`.
#[derive(Clone)]
pub struct SystemPromptService {
    db_path: Option<Arc<str>>,
    fallback: Arc<str>,
}

impl SystemPromptService {
    /// Store-backed service; falls back to `fallback` when no active version.
    pub fn with_store(db_path: impl Into<Arc<str>>, fallback: impl Into<String>) -> Self {
        Self {
            db_path: Some(db_path.into()),
            fallback: fallback.into().into(),
        }
    }

    /// Pure in-memory service (tests / defaults).
    pub fn fixed(prompt: impl Into<String>) -> Self {
        Self {
            db_path: None,
            fallback: prompt.into().into(),
        }
    }

    pub async fn active(&self) -> Result<String, StoreError> {
        let Some(path) = &self.db_path else {
            return Ok(self.fallback.to_string());
        };
        let path = path.clone();
        let fallback = self.fallback.clone();
        tokio::task::spawn_blocking(move || {
            let conn = Db::open(&path)?;
            crate::store::migrations::run(&conn.0)?;
            Ok(repos::prompts::get_active(&conn.0, "system_prompt")
                .map(|v| v.content)
                .unwrap_or_else(|_| fallback.to_string()))
        })
        .await
        .map_err(join_err)?
    }
}

fn join_err(e: tokio::task::JoinError) -> StoreError {
    StoreError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

pub struct SystemPromptPlugin {
    service: SystemPromptService,
}

impl SystemPromptPlugin {
    pub fn new(service: SystemPromptService) -> Self {
        Self { service }
    }
}

#[async_trait]
impl Plugin for SystemPromptPlugin {
    fn id(&self) -> &str {
        "system_prompt"
    }

    async fn init(&self, ctx: &Context) -> Result<(), HarnessError> {
        ctx.register_service("system_prompt", "", Arc::new(self.service.clone()))
            .await
    }
}
