//! Long-term memory plugin: remember / recall across sessions.

use std::sync::Arc;

use crate::domain::MemoryEntry;
use crate::harness::{Context, HarnessError, Plugin};
use crate::store::repos;
use crate::store::StoreError;
use async_trait::async_trait;

use super::super::domain;

/// Service surface registered as `"memory"`.
#[derive(Clone)]
pub struct MemoryService {
    db_path: Arc<str>,
}

impl MemoryService {
    pub fn new(db_path: impl Into<Arc<str>>) -> Self {
        Self {
            db_path: db_path.into(),
        }
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
        };
        self.with_db({
            let entry = entry.clone();
            move |c| repos::memory::insert(c, &entry)
        })
        .await?;
        Ok(entry)
    }

    /// Keyword + tag retrieval for context injection.
    pub async fn recall(
        &self,
        keyword: Option<String>,
        tag: Option<String>,
        limit: u32,
    ) -> Result<Vec<MemoryEntry>, StoreError> {
        self.with_db(move |c| repos::memory::search(c, keyword.as_deref(), tag.as_deref(), limit))
            .await
    }

    pub async fn user_profile(&self) -> Result<Vec<MemoryEntry>, StoreError> {
        self.with_db(repos::memory::list_user_profile).await
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
