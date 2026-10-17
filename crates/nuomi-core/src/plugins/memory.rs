//! Long-term memory plugin: remember / recall across sessions.

use std::sync::Arc;

use crate::domain::{EvolutionSettings, MemoryEntry, MemoryPolicy, RetrievalStrategy};
use crate::harness::{Context, HarnessError, Plugin};
use crate::store::repos;
use crate::store::StoreError;
use async_trait::async_trait;
use rusqlite::params;

use super::super::domain;

/// `app_settings` key holding the serialized [`EvolutionSettings`] JSON.
const EVOLUTION_SETTINGS_KEY: &str = "evolution_settings";

/// Loads the memory policy from `app_settings.evolution_settings`.
/// Returns the default policy when the row is missing or malformed,
/// so memory never breaks boot over a settings parse failure.
fn load_memory_policy(conn: &rusqlite::Connection) -> MemoryPolicy {
    let Ok(Some(raw)) = repos::settings::get(conn, EVOLUTION_SETTINGS_KEY) else {
        return MemoryPolicy::default();
    };
    serde_json::from_str::<EvolutionSettings>(&raw)
        .map(|s| s.memory_policy)
        .unwrap_or_default()
}

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
        let policy_override = self.memory_policy.clone();
        self.with_db({
            let entry = entry.clone();
            move |c| -> Result<(), StoreError> {
                repos::memory::insert(c, &entry)?;
                // Enforce retention in the same connection: delete entries
                // older than the policy's retention window. The policy is
                // read live from app_settings so settings changes take
                // effect without a restart (ADR 0019 decision 4).
                let retention_days = policy_override
                    .map(|p| p.retention_days)
                    .unwrap_or_else(|| load_memory_policy(c).retention_days);
                let cutoff = domain::now_ms() - (retention_days as i64) * 86_400_000;
                let _ = c.execute(
                    "DELETE FROM memory_entries WHERE created_at < ?1",
                    params![cutoff],
                );
                Ok(())
            }
        })
        .await?;
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
        let policy_override = self.memory_policy.clone();
        self.with_db(move |c| {
            let retrieval = policy_override
                .map(|p| p.retrieval)
                .unwrap_or_else(|| load_memory_policy(c).retrieval);
            match retrieval {
                RetrievalStrategy::Keyword => {
                    // LIKE matches a single substring; use the first token so
                    // multi-word queries still return relevant hits.
                    let first_word = keyword
                        .as_deref()
                        .and_then(|s| s.split_whitespace().next())
                        .map(str::to_string);
                    repos::memory::search(
                        c,
                        first_word.as_deref(),
                        tag.as_deref(),
                        role_id.as_deref(),
                        limit,
                    )
                }
                RetrievalStrategy::Semantic | RetrievalStrategy::Hybrid => {
                    if let Some(query) = keyword.as_deref() {
                        repos::memory::search_fts(c, query, role_id.as_deref(), limit)
                            .map(|hits| hits.into_iter().map(|h| h.entry).collect())
                    } else {
                        repos::memory::search(c, None, tag.as_deref(), role_id.as_deref(), limit)
                    }
                }
            }
        })
        .await
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
            let n = c.execute(
                "DELETE FROM memory_entries WHERE created_at < ?1",
                params![cutoff],
            )?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;
    use rusqlite::Connection;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    #[test]
    fn load_memory_policy_defaults_when_unset() {
        let conn = db();
        let p = load_memory_policy(&conn);
        assert_eq!(p.retention_days, 90);
        assert_eq!(p.retrieval, RetrievalStrategy::Keyword);
    }

    #[test]
    fn load_memory_policy_reads_evolution_settings() {
        let conn = db();
        let settings = EvolutionSettings {
            memory_policy: MemoryPolicy {
                retention_days: 30,
                retrieval: RetrievalStrategy::Semantic,
            },
            ..Default::default()
        };
        let raw = serde_json::to_string(&settings).unwrap();
        repos::settings::set(&conn, EVOLUTION_SETTINGS_KEY, &raw).unwrap();
        let p = load_memory_policy(&conn);
        assert_eq!(p.retention_days, 30);
        assert_eq!(p.retrieval, RetrievalStrategy::Semantic);
    }

    #[test]
    fn load_memory_policy_defaults_on_malformed_json() {
        let conn = db();
        repos::settings::set(&conn, EVOLUTION_SETTINGS_KEY, "not json").unwrap();
        let p = load_memory_policy(&conn);
        assert_eq!(p.retention_days, 90);
        assert_eq!(p.retrieval, RetrievalStrategy::Keyword);
    }
}
