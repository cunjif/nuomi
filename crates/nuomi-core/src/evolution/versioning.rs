//! Prompt version management: wraps `store::repos::prompts` with automatic
//! version allocation — candidates never overwrite the active prompt until
/// explicitly activated (AC12).
use std::sync::Arc;

use crate::domain::{new_id, now_ms, PromptStatus, PromptVersion};
use crate::store::{repos, Db};

use super::reflection::PromptCandidate;
use super::EvolutionError;

#[derive(Clone)]
pub struct PromptVersionManager {
    db_path: Arc<str>,
}

impl PromptVersionManager {
    pub fn new(db_path: impl Into<Arc<str>>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }

    async fn with_db<T: Send + 'static>(
        &self,
        f: impl FnOnce(&rusqlite::Connection) -> Result<T, crate::store::StoreError> + Send + 'static,
    ) -> Result<T, EvolutionError> {
        let path = self.db_path.clone();
        tokio::task::spawn_blocking(move || {
            let db = Db::open(&path)?;
            crate::store::migrations::run(&db.0)?;
            f(&db.0)
        })
        .await
        .map_err(|e| EvolutionError::Store(e.to_string()))?
        .map_err(|e| EvolutionError::Store(e.to_string()))
    }

    /// Stores `candidate` as the next version of `plugin` (max existing + 1).
    pub async fn propose_candidate(
        &self,
        plugin: &str,
        candidate: PromptCandidate,
    ) -> Result<PromptVersion, EvolutionError> {
        let plugin = plugin.to_string();
        self.with_db(move |conn| {
            let existing = repos::prompts::list_by_plugin(conn, &plugin)?;
            let next = existing.iter().map(|v| v.version).max().unwrap_or(0) + 1;
            let parent =
                candidate
                    .parent_version
                    .or_else(|| if next > 1 { Some(next - 1) } else { None });
            let version = PromptVersion {
                id: new_id(),
                plugin: plugin.clone(),
                version: next,
                status: PromptStatus::Candidate,
                content: candidate.content,
                diff_text: Some(candidate.diff_text),
                parent_version: parent,
                activated_at: None,
                created_at: now_ms(),
            };
            repos::prompts::insert_candidate(conn, &version)?;
            Ok(version)
        })
        .await
    }

    pub async fn activate(&self, plugin: &str, version: i64) -> Result<(), EvolutionError> {
        let plugin = plugin.to_string();
        self.with_db(move |conn| repos::prompts::activate(conn, &plugin, version, now_ms()))
            .await
    }

    pub async fn active(&self, plugin: &str) -> Result<PromptVersion, EvolutionError> {
        let plugin = plugin.to_string();
        self.with_db(move |conn| repos::prompts::get_active(conn, &plugin))
            .await
    }

    pub async fn list(&self, plugin: &str) -> Result<Vec<PromptVersion>, EvolutionError> {
        let plugin = plugin.to_string();
        self.with_db(move |conn| repos::prompts::list_by_plugin(conn, &plugin))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evolution::trajectory::TrajectorySummary;

    fn summary(id: &str) -> TrajectorySummary {
        TrajectorySummary {
            session_id: id.into(),
            turns: 1,
            tool_calls: 0,
            outcomes: vec!["hi".into()],
        }
    }

    fn json_response(content: &str, diff: &str) -> crate::providers::ChatResponse {
        crate::providers::FakeLlm::response(
            &serde_json::json!({ "content": content, "diff": diff }).to_string(),
        )
    }

    /// AC12 end-to-end: two trajectories → reflect → propose → activate →
    /// supersede, with versions stored and the old one retired.
    #[tokio::test]
    async fn propose_activate_supersede_full_chain() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("evo.db");
        let manager = PromptVersionManager::new(db_path.to_string_lossy().to_string());

        let llm = std::sync::Arc::new(crate::providers::FakeLlm::new(
            "fake",
            vec![
                json_response("prompt v2 body", "+concise style"),
                json_response("prompt v3 body", "+tool guidance"),
            ],
        ));
        let reflector = crate::evolution::Reflector::new(llm);
        let input = crate::evolution::ReflectionInput {
            trajectories: vec![summary("s1"), summary("s2")],
            current_prompt: "You are nuomi.".into(),
            ..Default::default()
        };

        // Candidate 1: proposed and explicitly activated.
        let c1 = reflector.reflect(&input).await.unwrap();
        let v1 = manager
            .propose_candidate("system_prompt", c1)
            .await
            .unwrap();
        assert_eq!(v1.version, 1);
        assert_eq!(v1.status, PromptStatus::Candidate);
        assert_eq!(v1.parent_version, None);

        // Not activated yet → no active version.
        assert!(manager.active("system_prompt").await.is_err());

        manager.activate("system_prompt", 1).await.unwrap();
        let active = manager.active("system_prompt").await.unwrap();
        assert_eq!((active.version, active.status), (1, PromptStatus::Active));
        assert_eq!(active.content, "prompt v2 body");

        // Candidate 2 supersedes v1.
        let c2 = reflector.reflect(&input).await.unwrap();
        let v2 = manager
            .propose_candidate("system_prompt", c2)
            .await
            .unwrap();
        assert_eq!(v2.version, 2);
        assert_eq!(v2.parent_version, Some(1));
        assert_eq!(v2.diff_text.as_deref(), Some("+tool guidance"));

        manager.activate("system_prompt", 2).await.unwrap();
        let active = manager.active("system_prompt").await.unwrap();
        assert_eq!(active.version, 2);

        let all = manager.list("system_prompt").await.unwrap();
        assert_eq!(all.len(), 2);
        let retired_v1 = all.iter().find(|v| v.version == 1).unwrap();
        assert_eq!(retired_v1.status, PromptStatus::Retired);
    }
}
