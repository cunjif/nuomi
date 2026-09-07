//! Prompt version management: wraps `store::repos::prompts` with automatic
//! version allocation — candidates never overwrite the active prompt until
/// explicitly activated (AC12).
///
/// Base-prompt immutability invariant: the active ("base") prompt is *never*
/// mutated in place. The only write paths are (a) `propose_candidate`, which
/// appends a fresh candidate row, and (b) `activate`, which flips statuses
/// through the candidate → active → retired state machine. The store repo
/// exposes no content-update API by design; a protection test pins this.
use std::sync::Arc;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::domain::{new_id, now_ms, PromptStatus, PromptVersion};
use crate::store::{repos, Db, StoreError};

use super::journal::{audit, EvolutionJournal, JournalKind};
use super::reflection::PromptCandidate;
use super::EvolutionError;

/// Optimistic-concurrency baseline captured at planning time
/// ([`PromptVersionManager::plan_baseline`]) and checked again at apply time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyBaseline {
    pub plugin: String,
    /// id of the active version observed at planning time; `None` when no
    /// version was active yet.
    pub active_id: Option<String>,
}

/// Before/after snapshot of one successful candidate application. Kept in an
/// in-memory registry, exported via serde, and persisted as an append-only
/// `prompt_applied` EventRecord — no new table, no migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplySnapshot {
    pub plugin: String,
    /// The candidate version number that was activated.
    pub applied_version: i64,
    /// Previously active version (`None` when nothing was active before).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<PromptVersion>,
    /// The version active after the apply.
    pub after: PromptVersion,
    pub applied_at: i64,
}

#[derive(Clone)]
pub struct PromptVersionManager {
    db_path: Arc<str>,
    /// In-memory before/after snapshots of successful applies (newest last).
    snapshots: Arc<Mutex<Vec<ApplySnapshot>>>,
    /// Optional Harness Journal: when set, every versioning action
    /// (proposal, baseline, apply) lands as an immutable audit entry.
    journal: Option<Arc<EvolutionJournal>>,
}

impl PromptVersionManager {
    pub fn new(db_path: impl Into<Arc<str>>) -> Self {
        Self {
            db_path: db_path.into(),
            snapshots: Arc::new(Mutex::new(Vec::new())),
            journal: None,
        }
    }

    /// Attaches the Harness Journal for audit instrumentation.
    pub fn with_journal(mut self, journal: Arc<EvolutionJournal>) -> Self {
        self.journal = Some(journal);
        self
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
        let audit_plugin = plugin.clone();
        let version = self
            .with_db(move |conn| {
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
            .await?;
        audit(
            &self.journal,
            &audit_plugin,
            JournalKind::ProposalGenerated {
                proposal_ref: version.id.clone(),
            },
            "versioning",
            format!("candidate v{} stored for '{audit_plugin}'", version.version),
            vec![version.id.clone()],
            serde_json::json!({ "version": version }),
        );
        Ok(version)
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

    /// Captures the current active version id as an apply baseline. Call at
    /// planning time; [`Self::apply_candidate`] rejects the apply when the
    /// active version has drifted since.
    pub async fn plan_baseline(&self, plugin: &str) -> Result<ApplyBaseline, EvolutionError> {
        let plugin = plugin.to_string();
        let baseline = self
            .with_db(move |conn| {
                let active_id = match repos::prompts::get_active(conn, &plugin) {
                    Ok(active) => Some(active.id),
                    Err(StoreError::NotFound { .. }) => None,
                    Err(other) => return Err(other),
                };
                Ok(ApplyBaseline { plugin, active_id })
            })
            .await?;
        audit(
            &self.journal,
            &baseline.plugin,
            JournalKind::BaselineCaptured {
                digest: baseline
                    .active_id
                    .clone()
                    .unwrap_or_else(|| "<none>".into()),
            },
            "versioning",
            format!("apply baseline captured for '{}'", baseline.plugin),
            baseline
                .active_id
                .clone()
                .map(|id| vec![id])
                .unwrap_or_default(),
            serde_json::json!({ "plugin": baseline.plugin }),
        );
        Ok(baseline)
    }

    /// Applies a candidate version (activates it, retiring the previous
    /// active) under optimistic concurrency: the apply is rejected unless the
    /// active version still matches `baseline.active_id` — the error carries
    /// both version ids so the conflict is diagnosable.
    ///
    /// On success a before/after [`ApplySnapshot`] is recorded: in-memory
    /// registry, tracing log, and an append-only `prompt_applied` EventRecord
    /// (payload JSON only — no schema change).
    pub async fn apply_candidate(
        &self,
        plugin: &str,
        version: i64,
        baseline: ApplyBaseline,
    ) -> Result<ApplySnapshot, EvolutionError> {
        let plugin = plugin.to_string();
        let db_plugin = plugin.clone();
        // The closure is typed to StoreError, so a baseline conflict is
        // carried out as an outcome enum and mapped to the conflict error
        // afterwards — the optimistic-concurrency check stays inside the
        // same DB closure as the activation it guards.
        enum ApplyOutcome {
            Conflict {
                expected_id: Option<String>,
                actual_id: Option<String>,
            },
            Applied(Box<ApplySnapshot>),
        }
        let snapshot = self
            .with_db(move |conn| {
                let plugin = db_plugin;
                let current = match repos::prompts::get_active(conn, &plugin) {
                    Ok(active) => Some(active),
                    Err(StoreError::NotFound { .. }) => None,
                    Err(other) => return Err(other),
                };
                let current_id = current.as_ref().map(|v| v.id.clone());
                if current_id != baseline.active_id {
                    return Ok(ApplyOutcome::Conflict {
                        expected_id: baseline.active_id,
                        actual_id: current_id,
                    });
                }
                repos::prompts::activate(conn, &plugin, version, now_ms())?;
                let after = repos::prompts::get_active(conn, &plugin)?;
                let snapshot = ApplySnapshot {
                    plugin: plugin.clone(),
                    applied_version: version,
                    before: current,
                    after,
                    applied_at: now_ms(),
                };
                // Append-only audit trail alongside the version rows.
                let payload = serde_json::to_value(&snapshot)?;
                repos::events::append(
                    conn,
                    "prompt_version",
                    &plugin,
                    "prompt_applied",
                    &payload,
                    now_ms(),
                )?;
                Ok(ApplyOutcome::Applied(Box::new(snapshot)))
            })
            .await?;
        let snapshot = match snapshot {
            ApplyOutcome::Conflict {
                expected_id,
                actual_id,
            } => {
                return Err(EvolutionError::BaselineConflict {
                    plugin,
                    expected_id: expected_id.unwrap_or_else(|| "<none>".to_string()),
                    actual_id: actual_id.unwrap_or_else(|| "<none>".to_string()),
                });
            }
            ApplyOutcome::Applied(snapshot) => *snapshot,
        };

        self.snapshots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(snapshot.clone());
        tracing::info!(
            plugin = %snapshot.plugin,
            applied_version = snapshot.applied_version,
            before_id = ?snapshot.before.as_ref().map(|v| v.id.as_str()),
            after_id = %snapshot.after.id,
            "prompt candidate applied; before/after snapshot recorded"
        );
        // Audit entry with the full before/after snapshot in the payload —
        // this is what time-travel rollback replays from.
        let mut evidence = Vec::new();
        if let Some(before) = &snapshot.before {
            evidence.push(before.id.clone());
        }
        evidence.push(snapshot.after.id.clone());
        audit(
            &self.journal,
            &snapshot.plugin,
            JournalKind::Applied {
                before_ref: snapshot.before.as_ref().map(|v| v.id.clone()),
                after_ref: snapshot.after.id.clone(),
            },
            "versioning",
            format!("applied v{} to '{plugin}'", snapshot.applied_version),
            evidence,
            serde_json::json!({ "snapshot": snapshot }),
        );
        Ok(snapshot)
    }

    /// Rolls back a previously applied candidate by re-proposing the
    /// snapshot's `before` content as a fresh candidate and activating it:
    /// history is append-only, so a rollback is a new forward version, never
    /// an in-place mutation.
    pub async fn rollback(
        &self,
        snapshot: &ApplySnapshot,
    ) -> Result<PromptVersion, EvolutionError> {
        let before = snapshot
            .before
            .as_ref()
            .ok_or(EvolutionError::NothingToRollBack)?;
        let candidate = PromptCandidate {
            content: before.content.clone(),
            diff_text: format!(
                "rollback of v{} to the content of v{}",
                snapshot.applied_version, before.version
            ),
            parent_version: None,
        };
        let restored = self.propose_candidate(&snapshot.plugin, candidate).await?;
        self.activate(&snapshot.plugin, restored.version).await?;
        tracing::info!(
            plugin = %snapshot.plugin,
            restored_version = restored.version,
            rolled_back_version = snapshot.applied_version,
            "prompt rollback applied as a new forward version"
        );
        Ok(restored)
    }

    /// In-memory before/after snapshots of every successful apply, oldest
    /// first.
    pub fn snapshots(&self) -> Vec<ApplySnapshot> {
        self.snapshots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// serde export of all recorded snapshots (archival / inspection).
    pub fn snapshots_json(&self) -> Result<String, EvolutionError> {
        let guard = self
            .snapshots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        serde_json::to_string(&*guard)
            .map_err(|e| EvolutionError::Store(format!("snapshot export failed: {e}")))
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

    fn candidate(content: &str, diff: &str) -> PromptCandidate {
        PromptCandidate {
            content: content.to_string(),
            diff_text: diff.to_string(),
            parent_version: None,
        }
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

    /// Base-prompt immutability protection: activating a new version never
    /// mutates a previous version's row in place — only its status moves
    /// through the state machine; id, content, timestamps stay frozen.
    #[tokio::test]
    async fn active_base_prompt_is_never_mutated_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let manager = PromptVersionManager::new(dir.path().join("evo.db").to_string_lossy());

        let v1 = manager
            .propose_candidate("system_prompt", candidate("base body", "init"))
            .await
            .unwrap();
        manager.activate("system_prompt", v1.version).await.unwrap();
        let base_before = manager.active("system_prompt").await.unwrap();

        let v2 = manager
            .propose_candidate("system_prompt", candidate("new body", "+tweaks"))
            .await
            .unwrap();
        manager.activate("system_prompt", v2.version).await.unwrap();

        let all = manager.list("system_prompt").await.unwrap();
        let old = all.iter().find(|v| v.version == 1).unwrap();
        assert_eq!(old.id, base_before.id);
        assert_eq!(old.content, base_before.content);
        assert_eq!(old.created_at, base_before.created_at);
        assert_eq!(old.activated_at, base_before.activated_at);
        // Only the status transitioned; content history is immutable.
        assert_eq!(old.status, PromptStatus::Retired);
    }

    #[tokio::test]
    async fn apply_candidate_records_snapshot_and_event() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("evo.db");
        let manager = PromptVersionManager::new(db_path.to_string_lossy());

        let v1 = manager
            .propose_candidate("system_prompt", candidate("base body", "init"))
            .await
            .unwrap();
        manager.activate("system_prompt", v1.version).await.unwrap();
        let baseline = manager.plan_baseline("system_prompt").await.unwrap();
        assert_eq!(baseline.active_id.as_deref(), Some(v1.id.as_str()));

        let v2 = manager
            .propose_candidate("system_prompt", candidate("next body", "+tweaks"))
            .await
            .unwrap();
        let snapshot = manager
            .apply_candidate("system_prompt", v2.version, baseline)
            .await
            .unwrap();

        assert_eq!(snapshot.applied_version, 2);
        assert_eq!(snapshot.before.as_ref().unwrap().content, "base body");
        assert_eq!(snapshot.after.status, PromptStatus::Active);
        assert_eq!(snapshot.after.content, "next body");

        // In-memory registry + serde export.
        assert_eq!(manager.snapshots().len(), 1);
        let exported = manager.snapshots_json().unwrap();
        assert!(exported.contains("base body"), "export: {exported}");
        assert!(exported.contains("next body"), "export: {exported}");

        // The snapshot landed as an append-only EventRecord.
        let conn = rusqlite::Connection::open(db_path.as_path()).unwrap();
        let events = crate::store::repos::events::list_by_aggregate(
            &conn,
            "prompt_version",
            "system_prompt",
            None,
        )
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, "prompt_applied");
        assert_eq!(
            events[0].payload["after"]["content"],
            serde_json::json!("next body")
        );
    }

    /// Optimistic concurrency: applying against a stale baseline is rejected
    /// and the error names both the expected and the actual version id.
    #[tokio::test]
    async fn apply_rejects_concurrent_modification_naming_both_ids() {
        let dir = tempfile::tempdir().unwrap();
        let manager = PromptVersionManager::new(dir.path().join("evo.db").to_string_lossy());

        let v1 = manager
            .propose_candidate("system_prompt", candidate("base body", "init"))
            .await
            .unwrap();
        manager.activate("system_prompt", v1.version).await.unwrap();
        let baseline = manager.plan_baseline("system_prompt").await.unwrap();

        // Concurrent writer: activates v3 while our apply is in flight.
        let v2 = manager
            .propose_candidate("system_prompt", candidate("ours", "ours"))
            .await
            .unwrap();
        let v3 = manager
            .propose_candidate("system_prompt", candidate("theirs", "theirs"))
            .await
            .unwrap();
        manager.activate("system_prompt", v3.version).await.unwrap();

        let err = manager
            .apply_candidate("system_prompt", v2.version, baseline)
            .await
            .unwrap_err();
        assert!(
            matches!(err, EvolutionError::BaselineConflict { .. }),
            "{err}"
        );
        let message = err.to_string();
        assert!(message.contains(&v1.id), "missing expected id: {message}");
        assert!(message.contains(&v3.id), "missing actual id: {message}");

        // With a fresh baseline the same candidate applies cleanly.
        let fresh = manager.plan_baseline("system_prompt").await.unwrap();
        let snapshot = manager
            .apply_candidate("system_prompt", v2.version, fresh)
            .await
            .unwrap();
        assert_eq!(snapshot.after.version, 2);
    }

    /// Rollback restores the snapshot's `before` content — as a new forward
    /// version, never by mutating history.
    #[tokio::test]
    async fn rollback_restores_previous_content_as_new_version() {
        let dir = tempfile::tempdir().unwrap();
        let manager = PromptVersionManager::new(dir.path().join("evo.db").to_string_lossy());

        let v1 = manager
            .propose_candidate("system_prompt", candidate("safe body", "init"))
            .await
            .unwrap();
        manager.activate("system_prompt", v1.version).await.unwrap();
        let baseline = manager.plan_baseline("system_prompt").await.unwrap();
        let v2 = manager
            .propose_candidate("system_prompt", candidate("risky body", "+risky"))
            .await
            .unwrap();
        let snapshot = manager
            .apply_candidate("system_prompt", v2.version, baseline)
            .await
            .unwrap();

        let restored = manager.rollback(&snapshot).await.unwrap();
        assert_eq!(restored.content, "safe body");
        let active = manager.active("system_prompt").await.unwrap();
        assert_eq!(active.version, 3);
        assert_eq!(active.content, "safe body");
        // v2 stays in history, retired — nothing was deleted or overwritten.
        let all = manager.list("system_prompt").await.unwrap();
        assert_eq!(all.len(), 3);
        let bad = all.iter().find(|v| v.version == 2).unwrap();
        assert_eq!(bad.status, PromptStatus::Retired);
        assert_eq!(bad.content, "risky body");
    }
}
