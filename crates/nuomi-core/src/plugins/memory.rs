//! Long-term memory plugin: remember / recall across sessions.

use std::sync::Arc;

use crate::domain::{MemoryEntry, MemoryPolicy, RetrievalStrategy};
use crate::harness::{Context, HarnessError, Plugin};
use crate::store::repos;
use crate::store::StoreError;
use async_trait::async_trait;
use rusqlite::params;

use super::super::domain;

/// Service surface registered as `"memory"`.
#[derive(Clone)]
pub struct MemoryService {
    db_path: Arc<str>,
    /// Persistent-memory policy (retention + retrieval strategy).
    memory_policy: Option<MemoryPolicy>,
}

impl MemoryService {
    pub fn new(db_path: impl Into<Arc<str>>) -> Self {
        Self {
            db_path: db_path.into(),
            memory_policy: None,
        }
    }

    /// Attaches a persistent-memory policy. The policy controls retrieval
    /// strategy (keyword vs FTS5) and retention enforcement.
    pub fn with_memory_policy(mut self, policy: MemoryPolicy) -> Self {
        self.memory_policy = Some(policy);
        self
    }

    /// Returns the attached memory policy, if any.
    pub fn memory_policy(&self) -> Option<&MemoryPolicy> {
        self.memory_policy.as_ref()
    }

    /// Runs `f` with a fresh blocking connection to the shared database.
    async fn with_db<T: Send + 'static>(
        &self,
        f: impl FnOnce(&rusqlite::Connection) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let path = self.db_path.clone();
        tokio::task::spawn_blocking(move || {
            let conn = crate::store::Db::open(&path)?;
            migrations_ok(&conn.0)?;
            f(&conn.0)
        })
        .await
        .map_err(|e| StoreError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e))))?
    }

    pub async fn remember(
        &self,
        content: String,
        source_session_id: Option<String>,
        tags: Vec<String>,
        kind: &str,
        user_profile: bool,
        role_id: Option<String>,
    ) -> Result<MemoryEntry, StoreError> {
        let now = domain::now_ms();
        let entry = MemoryEntry {
            id: domain::new_id(),
            content,
            source_session_id,
            tags,
            kind: kind.to_string(),
            user_profile,
            created_at: now,
            updated_at: now,
            role_id,
        };
        self.with_db({
            let entry = entry.clone();
            move |c| repos::memory::insert(c, &entry)
        })
        .await?;
        // Trigger retention enforcement on write (low-frequency, acceptable).
        if let Some(policy) = &self.memory_policy {
            let _ = self.enforce_retention(policy.retention_days).await;
        }
        Ok(entry)
    }

    /// Keyword + tag retrieval for context injection.
    /// Dispatches based on `memory_policy.retrieval`:
    /// - `Keyword` → LIKE-based `search`
    /// - `Semantic`/`Hybrid` → FTS5 `search_fts` (falls back to LIKE for short queries)
    pub async fn recall(
        &self,
        keyword: Option<String>,
        tag: Option<String>,
        role_id: Option<String>,
        limit: u32,
    ) -> Result<Vec<MemoryEntry>, StoreError> {
        let retrieval = self
            .memory_policy
            .as_ref()
            .map(|p| p.retrieval)
            .unwrap_or(RetrievalStrategy::Keyword);
        match retrieval {
            RetrievalStrategy::Keyword => {
                self.with_db(move |c| {
                    repos::memory::search(c, keyword.as_deref(), tag.as_deref(), role_id.as_deref(), limit)
                })
                .await
            }
            RetrievalStrategy::Semantic | RetrievalStrategy::Hybrid => {
                if let Some(query) = &keyword {
                    let query = query.clone();
                    let role_id = role_id.clone();
                    self.with_db(move |c| {
                        repos::memory::search_fts(c, &query, role_id.as_deref(), limit)
                    })
                    .await
                    .map(|hits| hits.into_iter().map(|h| h.entry).collect())
                } else {
                    self.with_db(move |c| {
                        repos::memory::search(c, None, tag.as_deref(), role_id.as_deref(), limit)
                    })
                    .await
                }
            }
        }
    }

    pub async fn user_profile(&self) -> Result<Vec<MemoryEntry>, StoreError> {
        self.with_db(repos::memory::list_user_profile).await
    }

    /// Lists memory entries with optional role_id filter.
    pub async fn list(
        &self,
        role_id: Option<String>,
        limit: u32,
    ) -> Result<Vec<MemoryEntry>, StoreError> {
        self.with_db(move |c| repos::memory::list(c, role_id.as_deref(), limit))
            .await
    }

    /// Deletes a memory entry by id.
    pub async fn delete(&self, id: String) -> Result<bool, StoreError> {
        self.with_db(move |c| repos::memory::delete(c, &id)).await
    }

    /// Deletes memory entries older than `retention_days`.
    /// Returns the number of rows deleted.
    pub async fn enforce_retention(&self, retention_days: u32) -> Result<u64, StoreError> {
        let cutoff = domain::now_ms() - (retention_days as i64) * 86_400_000;
        self.with_db(move |c| {
            let n = c.execute("DELETE FROM memory_entries WHERE created_at < ?1", params![cutoff])?;
            Ok(n as u64)
        })
        .await
    }
}

fn migrations_ok(conn: &rusqlite::Connection) -> Result<(), StoreError> {
    crate::store::migrations::run(conn)
}

pub struct MemoryPlugin {
    service: MemoryService,
}

impl MemoryPlugin {
    pub fn new(db_path: impl Into<Arc<str>>) -> Self {
        Self {
            service: MemoryService::new(db_path),
        }
    }
}

#[async_trait]
impl Plugin for MemoryPlugin {
    fn id(&self) -> &str {
        "memory"
    }

    async fn init(&self, ctx: &Context) -> Result<(), HarnessError> {
        ctx.register_service("memory", "memory", Arc::new(self.service.clone()))
            .await
    }
}
