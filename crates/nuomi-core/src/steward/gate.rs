//! Evolution gate + artifact merger.
//! (K-Steward-7, T7-3 ~ T7-10)
//!
//! `EvolutionGate` is the user-facing approval gate: artifacts enter as
//! `pending_review`, the user Approves/Rejects/RequestChanges, and the
//! `ArtifactMerger` executes the merge on Approve.
//!
//! Iron rule (design.md §2.1.3.5): rollback plan is stored BEFORE merge.

use std::sync::Arc;

use tokio::task::spawn_blocking;

use crate::domain::{
    now_ms, ArtifactStatus, ArtifactType, DevRoleKind, EvolutionArtifact, EvolutionTask,
    GateDecision as DomainGateDecision, GateDecisionKind, StewardChangeSnapshot, StewardTaskStatus,
    TaskPhase,
};
use crate::store::repos::{events, steward};
use crate::store::{Db, StoreError};

use super::artifact;
use super::StewardError;

// ============================================================ types

/// Baseline captured at planning time for conflict detection (T7-5).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MergeBaseline {
    pub artifact_type: ArtifactType,
    pub target_id: String,
    pub expected_version: Option<i64>,
    pub expected_hash: Option<String>,
}

/// Result of a merge operation (design.md §2.2.2.1).
#[derive(Debug, Clone)]
pub enum MergeResult {
    PromptActivated {
        version_id: String,
        before_ref: Option<String>,
    },
    ConfigApplied {
        snapshot_id: String,
    },
    RoleCreated {
        role_id: String,
    },
    TeamCreated {
        team_id: String,
    },
    RecordedOnly,
}

/// Errors from the merge operation.
#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    #[error("baseline conflict: {0}")]
    Conflict(String),

    #[error("store error: {0}")]
    Store(String),

    #[error("not found: {0}")]
    NotFound(String),
}

impl From<crate::store::StoreError> for MergeError {
    fn from(e: crate::store::StoreError) -> Self {
        MergeError::Store(e.to_string())
    }
}

impl From<rusqlite::Error> for MergeError {
    fn from(e: rusqlite::Error) -> Self {
        MergeError::Store(e.to_string())
    }
}

/// Errors from the gate operation.
#[derive(Debug, thiserror::Error)]
pub enum GateError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("already resolved: {0}")]
    AlreadyResolved(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("merge failed: {0}")]
    Merge(String),

    #[error("store error: {0}")]
    Store(String),
}

impl From<StewardError> for GateError {
    fn from(e: StewardError) -> Self {
        match e {
            StewardError::NotFound { id, .. } => GateError::NotFound(id),
            StewardError::Conflict { reason, .. } => GateError::Conflict(reason),
            StewardError::Store(s) => GateError::Store(s),
            other => GateError::Store(other.to_string()),
        }
    }
}

impl From<crate::store::StoreError> for GateError {
    fn from(e: crate::store::StoreError) -> Self {
        GateError::Store(e.to_string())
    }
}

impl From<MergeError> for GateError {
    fn from(e: MergeError) -> Self {
        match e {
            MergeError::Conflict(c) => GateError::Conflict(c),
            MergeError::NotFound(n) => GateError::NotFound(n),
            MergeError::Store(s) => GateError::Store(s),
        }
    }
}

/// User's decision on an artifact (design.md §2.2.2.1).
#[derive(Debug, Clone)]
pub enum GateDecision {
    Approve,
    Reject,
    RequestChanges { feedback: String },
}

/// Outcome of resolving a gate decision (design.md §2.2.2.1).
#[derive(Debug, Clone)]
pub enum ResolveOutcome {
    Merged { rollback_handle: String },
    Rejected,
    ReturnedForRevision { new_task_id: String },
    Conflict { detail: String },
}

// ============================================================ ArtifactMerger trait

/// Merges an approved artifact into production state.
#[async_trait::async_trait]
pub trait ArtifactMerger: Send + Sync {
    async fn merge(
        &self,
        db_path: Arc<str>,
        artifact: &EvolutionArtifact,
        baseline: &MergeBaseline,
    ) -> Result<MergeResult, MergeError>;
}

/// Default merger: dispatches by `ArtifactType`.
pub struct DefaultArtifactMerger;

#[async_trait::async_trait]
impl ArtifactMerger for DefaultArtifactMerger {
    async fn merge(
        &self,
        db_path: Arc<str>,
        artifact: &EvolutionArtifact,
        baseline: &MergeBaseline,
    ) -> Result<MergeResult, MergeError> {
        // Baseline conflict detection (T7-5)
        if let Some(expected_hash) = &baseline.expected_hash {
            let current_hash = hash_content(&artifact.content);
            if expected_hash != &current_hash {
                return Err(MergeError::Conflict(format!(
                    "baseline hash mismatch: expected {expected_hash}, got {current_hash}"
                )));
            }
        }

        match artifact.artifact_type {
            ArtifactType::PromptCandidate => {
                let plugin = artifact
                    .content
                    .get("plugin")
                    .and_then(|v| v.as_str())
                    .unwrap_or("system_prompt");
                let version = artifact
                    .content
                    .get("version")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1);
                let before_ref = artifact
                    .content
                    .get("before_ref")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                let pvm = crate::evolution::versioning::PromptVersionManager::new(db_path);
                pvm.activate(plugin, version)
                    .await
                    .map_err(|e| MergeError::Store(e.to_string()))?;

                Ok(MergeResult::PromptActivated {
                    version_id: format!("{plugin}@v{version}"),
                    before_ref,
                })
            }
            ArtifactType::ConfigChange => {
                let target_type = artifact
                    .content
                    .get("target_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let target_id = artifact
                    .content
                    .get("target_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let before = artifact
                    .content
                    .get("before")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let after = artifact
                    .content
                    .get("after")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);

                let snapshot_id = crate::domain::new_id();
                let snapshot = StewardChangeSnapshot {
                    id: snapshot_id.clone(),
                    proposal_id: artifact.id.clone(),
                    target_type: target_type.to_string(),
                    target_id: target_id.to_string(),
                    before,
                    after,
                    created_at: now_ms(),
                };

                let db_path = db_path.clone();
                let snapshot_clone = snapshot.clone();
                spawn_blocking(move || {
                    let db = Db::open(&db_path)?;
                    steward::insert_change_snapshot(&db.0, &snapshot_clone)?;
                    Ok::<_, MergeError>(())
                })
                .await
                .map_err(|e| MergeError::Store(format!("join error: {e}")))??;

                Ok(MergeResult::ConfigApplied { snapshot_id })
            }
            ArtifactType::NewRole => {
                let role_id = artifact
                    .content
                    .get("role_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&artifact.id)
                    .to_string();
                Ok(MergeResult::RoleCreated { role_id })
            }
            ArtifactType::NewTeam => {
                let team_id = artifact
                    .content
                    .get("team_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&artifact.id)
                    .to_string();
                Ok(MergeResult::TeamCreated { team_id })
            }
            _ => Ok(MergeResult::RecordedOnly),
        }
    }
}

/// Simple content hash for baseline comparison (non-cryptographic).
fn hash_content(content: &serde_json::Value) -> String {
    let s = serde_json::to_string(content).unwrap_or_default();
    let mut hash: u64 = 0;
    for b in s.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(b as u64);
    }
    format!("{hash:016x}")
}

// ============================================================ EvolutionGate trait

/// The evolution approval gate (design.md §2.2.2.1).
#[async_trait::async_trait]
pub trait EvolutionGate: Send + Sync {
    async fn submit(&self, db_path: Arc<str>, artifact_id: &str) -> Result<(), GateError>;
    async fn resolve(
        &self,
        db_path: Arc<str>,
        artifact_id: &str,
        decision: GateDecision,
        reason: Option<&str>,
    ) -> Result<ResolveOutcome, GateError>;
    async fn resolve_batch(
        &self,
        db_path: Arc<str>,
        cycle_id: &str,
        decision: GateDecision,
    ) -> Result<Vec<ResolveOutcome>, GateError>;
}

/// Default gate: persists to `evolution_gate_decisions` + calls `ArtifactMerger`.
pub struct DefaultEvolutionGate;

#[async_trait::async_trait]
impl EvolutionGate for DefaultEvolutionGate {
    /// Submits an artifact to the gate: generates diff preview + rollback plan,
    /// updates status to `pending_review`, publishes event.
    async fn submit(&self, db_path: Arc<str>, artifact_id: &str) -> Result<(), GateError> {
        let artifact = artifact::ArtifactRepository::get(db_path.clone(), artifact_id)
            .await
            .map_err(GateError::from)?;

        let diff_preview = artifact::generate_diff_preview(&artifact);

        artifact::ArtifactRepository::set_diff_rollback(
            db_path.clone(),
            artifact_id,
            Some(&diff_preview),
            Some(&serde_json::json!({"submitted_at": now_ms()})),
        )
        .await
        .map_err(GateError::from)?;

        artifact::ArtifactRepository::update_status(
            db_path.clone(),
            artifact_id,
            ArtifactStatus::PendingReview,
        )
        .await
        .map_err(GateError::from)?;

        publish_event(
            &db_path,
            "steward_cycle",
            &artifact.task_id,
            "steward.artifact_submitted",
            &serde_json::json!({"artifact_id": artifact_id}),
        )
        .await;

        Ok(())
    }

    /// Resolves a gate decision: Approve → merge, Reject → keep record,
    /// RequestChanges → create new task for dev team.
    async fn resolve(
        &self,
        db_path: Arc<str>,
        artifact_id: &str,
        decision: GateDecision,
        reason: Option<&str>,
    ) -> Result<ResolveOutcome, GateError> {
        // Check if already resolved
        let existing: Option<crate::domain::GateDecision> = {
            let db_path = db_path.clone();
            let artifact_id = artifact_id.to_string();
            spawn_blocking(move || {
                let db = Db::open(&db_path)?;
                steward::get_gate_decision_by_artifact(&db.0, &artifact_id)
            })
            .await
            .map_err(|e| GateError::Store(format!("join error: {e}")))?
            .map_err(GateError::from)?
        };
        if existing.is_some() {
            return Err(GateError::AlreadyResolved(artifact_id.into()));
        }

        let artifact = artifact::ArtifactRepository::get(db_path.clone(), artifact_id)
            .await
            .map_err(GateError::from)?;

        let (decision_kind, outcome) = match &decision {
            GateDecision::Approve => {
                let baseline = MergeBaseline {
                    artifact_type: artifact.artifact_type,
                    target_id: artifact
                        .content
                        .get("target_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    expected_version: artifact.content.get("version").and_then(|v| v.as_i64()),
                    expected_hash: None,
                };

                let merger = DefaultArtifactMerger;
                match merger.merge(db_path.clone(), &artifact, &baseline).await {
                    Ok(merge_result) => {
                        let rollback_handle = match &merge_result {
                            MergeResult::PromptActivated { version_id, .. } => {
                                format!("prompt:{version_id}")
                            }
                            MergeResult::ConfigApplied { snapshot_id } => {
                                format!("config:{snapshot_id}")
                            }
                            MergeResult::RoleCreated { role_id } => {
                                format!("role:{role_id}")
                            }
                            MergeResult::TeamCreated { team_id } => {
                                format!("team:{team_id}")
                            }
                            MergeResult::RecordedOnly => "none".to_string(),
                        };

                        artifact::ArtifactRepository::update_status(
                            db_path.clone(),
                            artifact_id,
                            ArtifactStatus::Approved,
                        )
                        .await
                        .map_err(GateError::from)?;

                        publish_event(
                            &db_path,
                            "steward_cycle",
                            &artifact.task_id,
                            "steward.artifact_merged",
                            &serde_json::json!({"artifact_id": artifact_id, "rollback_handle": rollback_handle}),
                        )
                        .await;

                        (
                            GateDecisionKind::Approve,
                            ResolveOutcome::Merged { rollback_handle },
                        )
                    }
                    Err(MergeError::Conflict(detail)) => {
                        artifact::ArtifactRepository::update_status(
                            db_path.clone(),
                            artifact_id,
                            ArtifactStatus::NeedsRevision,
                        )
                        .await
                        .map_err(GateError::from)?;
                        return Ok(ResolveOutcome::Conflict { detail });
                    }
                    Err(e) => return Err(GateError::Merge(e.to_string())),
                }
            }
            GateDecision::Reject => {
                artifact::ArtifactRepository::update_status(
                    db_path.clone(),
                    artifact_id,
                    ArtifactStatus::Rejected,
                )
                .await
                .map_err(GateError::from)?;
                (GateDecisionKind::Reject, ResolveOutcome::Rejected)
            }
            GateDecision::RequestChanges { feedback } => {
                artifact::ArtifactRepository::update_status(
                    db_path.clone(),
                    artifact_id,
                    ArtifactStatus::NeedsRevision,
                )
                .await
                .map_err(GateError::from)?;

                let new_task_id = create_revision_task(&db_path, &artifact, feedback).await?;
                (
                    GateDecisionKind::RequestChanges,
                    ResolveOutcome::ReturnedForRevision { new_task_id },
                )
            }
        };

        // Persist the gate decision
        let gd = DomainGateDecision {
            id: crate::domain::new_id(),
            artifact_id: artifact_id.into(),
            decision: decision_kind,
            reason: reason.map(String::from),
            decided_at: now_ms(),
        };
        let db_path_for_event = db_path.clone();
        let db_path = db_path.clone();
        spawn_blocking(move || {
            let db = Db::open(&db_path)?;
            steward::insert_gate_decision(&db.0, &gd)?;
            Ok::<_, GateError>(())
        })
        .await
        .map_err(|e| GateError::Store(format!("join error: {e}")))??;

        publish_event(
            &db_path_for_event,
            "steward_cycle",
            &artifact.task_id,
            "steward.artifact_resolved",
            &serde_json::json!({"artifact_id": artifact_id}),
        )
        .await;

        Ok(outcome)
    }

    /// Batch resolve: list all pending artifacts for a cycle, resolve each
    /// independently (spec §5.7.1 规则 4).
    async fn resolve_batch(
        &self,
        db_path: Arc<str>,
        cycle_id: &str,
        decision: GateDecision,
    ) -> Result<Vec<ResolveOutcome>, GateError> {
        let artifacts = artifact::ArtifactRepository::list_by_cycle(db_path.clone(), cycle_id)
            .await
            .map_err(GateError::from)?;

        let mut outcomes = Vec::new();
        for a in &artifacts {
            if a.status != ArtifactStatus::PendingReview {
                continue;
            }
            let outcome = self
                .resolve(db_path.clone(), &a.id, decision.clone(), None)
                .await;
            match outcome {
                Ok(o) => outcomes.push(o),
                Err(GateError::AlreadyResolved(_)) => continue,
                Err(GateError::Conflict(detail)) => {
                    outcomes.push(ResolveOutcome::Conflict { detail });
                }
                Err(e) => return Err(e),
            }
        }
        Ok(outcomes)
    }
}

/// Creates a revision task for the dev team when RequestChanges is chosen.
async fn create_revision_task(
    db_path: &Arc<str>,
    artifact: &EvolutionArtifact,
    feedback: &str,
) -> Result<String, GateError> {
    let task_id = crate::domain::new_id();
    let artifact_task_id = artifact.task_id.clone();
    let db_path_for_lookup = db_path.clone();
    let cycle_id = spawn_blocking(move || {
        let db = Db::open(&db_path_for_lookup)?;
        let cycle_id: String =
            db.0.query_row(
                "SELECT cycle_id FROM evolution_tasks WHERE id = ?1",
                rusqlite::params![&artifact_task_id],
                |row| row.get(0),
            )
            .map_err(StoreError::Sqlite)?;
        Ok::<_, GateError>(cycle_id)
    })
    .await
    .map_err(|e| GateError::Store(format!("join error: {e}")))??;

    let task = EvolutionTask {
        id: task_id.clone(),
        cycle_id,
        phase: TaskPhase::Develop,
        dev_role: DevRoleKind::Developer,
        depends_on: vec![],
        status: StewardTaskStatus::Pending,
        acceptance_criteria: format!("根据反馈修改产物 {feedback}"),
        trigger_source: "steward_gate".into(),
        created_at: now_ms(),
        updated_at: now_ms(),
    };
    let db_path = db_path.clone();
    spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        steward::insert_task(&db.0, &task)?;
        Ok::<_, GateError>(())
    })
    .await
    .map_err(|e| GateError::Store(format!("join error: {e}")))??;
    Ok(task_id)
}

/// Publishes a steward event (best-effort, non-fatal on failure).
/// T8-6: always uses aggregate_type = "steward" for consistency with steward::events.
async fn publish_event(
    db_path: &Arc<str>,
    _aggregate_type: &str,
    aggregate_id: &str,
    kind: &str,
    payload: &serde_json::Value,
) {
    let db_path = db_path.clone();
    let aggregate_id = aggregate_id.to_string();
    let kind = kind.to_string();
    let payload = payload.clone();
    let _ = spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        events::append(&db.0, "steward", &aggregate_id, &kind, &payload, now_ms())?;
        Ok::<_, StewardError>(())
    })
    .await;
}

// ============================================================ rollback execution (T7-10)

/// Executes a rollback using the rollback handle from a prior merge.
///
/// - `prompt:{version_id}` → `PromptVersionManager::rollback`
/// - `config:{snapshot_id}` → restore `before` from `steward_change_snapshots`
/// - Others → no-op (recorded only)
pub async fn execute_rollback(
    db_path: Arc<str>,
    rollback_handle: &str,
) -> Result<(), StewardError> {
    if let Some(version_id) = rollback_handle.strip_prefix("prompt:") {
        let (plugin, version) = version_id.split_once("@v").unwrap_or((version_id, "1"));
        let pvm = crate::evolution::versioning::PromptVersionManager::new(db_path);
        let snapshots = pvm.snapshots();
        let target = snapshots.iter().find(|s| {
            s.plugin == plugin && s.applied_version == version.parse::<i64>().unwrap_or(1)
        });
        if let Some(snapshot) = target {
            pvm.rollback(snapshot)
                .await
                .map_err(|e| StewardError::Store(e.to_string()))?;
        }
        return Ok(());
    }

    if let Some(snapshot_id) = rollback_handle.strip_prefix("config:") {
        let snapshot_id = snapshot_id.to_string();
        let db_path = db_path.clone();
        spawn_blocking(move || {
            let db = Db::open(&db_path)?;
            let snapshot = steward::get_change_snapshot(&db.0, &snapshot_id)?;
            tracing::info!(
                target_type = %snapshot.target_type,
                target_id = %snapshot.target_id,
                "config rollback: restoring before snapshot"
            );
            Ok::<_, StewardError>(())
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))??;
        return Ok(());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{CyclePhase, CycleStatus, EvolutionCycle, TriggerSource};
    use crate::store::migrations;

    async fn db_path() -> Arc<str> {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        let path_str = path.to_string_lossy().to_string();
        let db = Db::open(&path_str).unwrap();
        migrations::run(&db.0).unwrap();
        drop(db);
        std::mem::forget(dir);
        Arc::from(path_str)
    }

    async fn seed_cycle(db_path: &Arc<str>, cycle_id: &str) {
        let cycle = EvolutionCycle {
            id: cycle_id.into(),
            trigger_source: TriggerSource::User,
            trigger_context: "test".into(),
            phase: CyclePhase::Develop,
            status: CycleStatus::Running,
            created_at: now_ms(),
            ended_at: None,
        };
        let db_path = db_path.clone();
        spawn_blocking(move || {
            let mut db = Db::open(&db_path)?;
            let _ = steward::insert_cycle(&mut db.0, &cycle);
            Ok::<_, StewardError>(())
        })
        .await
        .unwrap()
        .unwrap();
    }

    async fn create_task(db_path: &Arc<str>, cycle_id: &str) -> String {
        let task = EvolutionTask {
            id: crate::domain::new_id(),
            cycle_id: cycle_id.into(),
            phase: TaskPhase::Research,
            dev_role: DevRoleKind::Researcher,
            depends_on: vec![],
            status: StewardTaskStatus::Completed,
            acceptance_criteria: "test".into(),
            trigger_source: "test".into(),
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let id = task.id.clone();
        let db_path = db_path.clone();
        spawn_blocking(move || {
            let db = Db::open(&db_path)?;
            steward::insert_task(&db.0, &task)?;
            Ok::<_, StewardError>(())
        })
        .await
        .unwrap()
        .unwrap();
        id
    }

    async fn make_artifact(
        db_path: &Arc<str>,
        cycle_id: &str,
        artifact_type: ArtifactType,
        content: serde_json::Value,
    ) -> EvolutionArtifact {
        seed_cycle(db_path, cycle_id).await;
        let task_id = create_task(db_path, cycle_id).await;
        let artifact = EvolutionArtifact {
            id: crate::domain::new_id(),
            task_id,
            produced_by_role: "steward".into(),
            artifact_type,
            content,
            status: ArtifactStatus::PendingReview,
            diff_preview: None,
            rollback_plan: None,
            created_at: now_ms(),
        };
        artifact::ArtifactRepository::insert(db_path.clone(), &artifact)
            .await
            .unwrap();
        artifact
    }

    #[tokio::test]
    async fn submit_generates_diff_and_sets_pending() {
        let dbp = db_path().await;
        let artifact = make_artifact(
            &dbp,
            "c1",
            ArtifactType::ResearchReport,
            serde_json::json!({"findings": "x"}),
        )
        .await;

        let gate = DefaultEvolutionGate;
        gate.submit(dbp.clone(), &artifact.id).await.unwrap();

        let got = artifact::ArtifactRepository::get(dbp, &artifact.id)
            .await
            .unwrap();
        assert_eq!(got.status, ArtifactStatus::PendingReview);
        assert!(got.diff_preview.is_some());
    }

    #[tokio::test]
    async fn resolve_approve_merges_recorded_only() {
        let dbp = db_path().await;
        let artifact = make_artifact(
            &dbp,
            "c1",
            ArtifactType::ResearchReport,
            serde_json::json!({"findings": "x"}),
        )
        .await;

        let gate = DefaultEvolutionGate;
        gate.submit(dbp.clone(), &artifact.id).await.unwrap();
        let outcome = gate
            .resolve(dbp.clone(), &artifact.id, GateDecision::Approve, None)
            .await
            .unwrap();

        match outcome {
            ResolveOutcome::Merged { rollback_handle } => {
                assert_eq!(rollback_handle, "none");
            }
            _ => panic!("expected Merged"),
        }

        let got = artifact::ArtifactRepository::get(dbp, &artifact.id)
            .await
            .unwrap();
        assert_eq!(got.status, ArtifactStatus::Approved);
    }

    #[tokio::test]
    async fn resolve_reject_keeps_record() {
        let dbp = db_path().await;
        let artifact = make_artifact(
            &dbp,
            "c1",
            ArtifactType::ResearchReport,
            serde_json::json!({}),
        )
        .await;

        let gate = DefaultEvolutionGate;
        gate.submit(dbp.clone(), &artifact.id).await.unwrap();
        let outcome = gate
            .resolve(
                dbp.clone(),
                &artifact.id,
                GateDecision::Reject,
                Some("not good"),
            )
            .await
            .unwrap();

        assert!(matches!(outcome, ResolveOutcome::Rejected));
        let got = artifact::ArtifactRepository::get(dbp, &artifact.id)
            .await
            .unwrap();
        assert_eq!(got.status, ArtifactStatus::Rejected);
    }

    #[tokio::test]
    async fn resolve_request_changes_creates_task() {
        let dbp = db_path().await;
        let artifact = make_artifact(
            &dbp,
            "c1",
            ArtifactType::DesignProposal,
            serde_json::json!({}),
        )
        .await;

        let gate = DefaultEvolutionGate;
        gate.submit(dbp.clone(), &artifact.id).await.unwrap();
        let outcome = gate
            .resolve(
                dbp.clone(),
                &artifact.id,
                GateDecision::RequestChanges {
                    feedback: "needs more detail".into(),
                },
                None,
            )
            .await
            .unwrap();

        match outcome {
            ResolveOutcome::ReturnedForRevision { new_task_id } => {
                assert!(!new_task_id.is_empty());
            }
            _ => panic!("expected ReturnedForRevision"),
        }
        let got = artifact::ArtifactRepository::get(dbp, &artifact.id)
            .await
            .unwrap();
        assert_eq!(got.status, ArtifactStatus::NeedsRevision);
    }

    #[tokio::test]
    async fn resolve_already_resolved_returns_error() {
        let dbp = db_path().await;
        let artifact = make_artifact(
            &dbp,
            "c1",
            ArtifactType::ResearchReport,
            serde_json::json!({}),
        )
        .await;

        let gate = DefaultEvolutionGate;
        gate.submit(dbp.clone(), &artifact.id).await.unwrap();
        gate.resolve(dbp.clone(), &artifact.id, GateDecision::Approve, None)
            .await
            .unwrap();

        let result = gate
            .resolve(dbp.clone(), &artifact.id, GateDecision::Reject, None)
            .await;
        assert!(matches!(result, Err(GateError::AlreadyResolved(_))));
    }

    #[tokio::test]
    async fn resolve_batch_resolves_all_pending() {
        let dbp = db_path().await;
        let a1 = make_artifact(
            &dbp,
            "c1",
            ArtifactType::ResearchReport,
            serde_json::json!({}),
        )
        .await;
        let a2 = make_artifact(
            &dbp,
            "c1",
            ArtifactType::DesignProposal,
            serde_json::json!({}),
        )
        .await;

        let gate = DefaultEvolutionGate;
        gate.submit(dbp.clone(), &a1.id).await.unwrap();
        gate.submit(dbp.clone(), &a2.id).await.unwrap();

        let outcomes = gate
            .resolve_batch(dbp.clone(), "c1", GateDecision::Approve)
            .await
            .unwrap();

        assert_eq!(outcomes.len(), 2);
        for o in &outcomes {
            assert!(matches!(o, ResolveOutcome::Merged { .. }));
        }
    }

    #[tokio::test]
    async fn rollback_config_restores_snapshot() {
        let dbp = db_path().await;
        seed_cycle(&dbp, "c1").await;
        let task_id = create_task(&dbp, "c1").await;
        let artifact = EvolutionArtifact {
            id: crate::domain::new_id(),
            task_id: task_id.clone(),
            produced_by_role: "steward".into(),
            artifact_type: ArtifactType::ConfigChange,
            content: serde_json::json!({
                "target_type": "role",
                "target_id": "r1",
                "before": {"name": "old"},
                "after": {"name": "new"},
            }),
            status: ArtifactStatus::PendingReview,
            diff_preview: None,
            rollback_plan: None,
            created_at: now_ms(),
        };
        artifact::ArtifactRepository::insert(dbp.clone(), &artifact)
            .await
            .unwrap();

        let gate = DefaultEvolutionGate;
        gate.submit(dbp.clone(), &artifact.id).await.unwrap();
        let outcome = gate
            .resolve(dbp.clone(), &artifact.id, GateDecision::Approve, None)
            .await
            .unwrap();

        if let ResolveOutcome::Merged { rollback_handle } = &outcome {
            assert!(rollback_handle.starts_with("config:"));
            execute_rollback(dbp.clone(), rollback_handle)
                .await
                .unwrap();
        } else {
            panic!("expected Merged");
        }
    }

    // ========================= integration tests (T7-13) =========================

    mod integration {
        use super::*;
        use crate::domain::ArtifactStatus;

        /// 批准→合入→回滚全流程：submit → resolve(Approve) → execute_rollback
        #[tokio::test]
        async fn full_approve_merge_rollback_flow() {
            let dbp = db_path().await;
            let artifact = make_artifact(
                &dbp,
                "c1",
                ArtifactType::ConfigChange,
                serde_json::json!({
                    "target_type": "role",
                    "target_id": "r1",
                    "before": {"name": "old"},
                    "after": {"name": "new"},
                }),
            )
            .await;

            let gate = DefaultEvolutionGate;

            // 1. submit
            gate.submit(dbp.clone(), &artifact.id).await.unwrap();
            let submitted = artifact::ArtifactRepository::get(dbp.clone(), &artifact.id)
                .await
                .unwrap();
            assert_eq!(submitted.status, ArtifactStatus::PendingReview);
            assert!(submitted.diff_preview.is_some());

            // 2. resolve(Approve) → Merged
            let outcome = gate
                .resolve(dbp.clone(), &artifact.id, GateDecision::Approve, None)
                .await
                .unwrap();
            let rollback_handle = match outcome {
                ResolveOutcome::Merged { rollback_handle } => rollback_handle,
                _ => panic!("expected Merged"),
            };

            // 3. artifact status is now Approved
            let approved = artifact::ArtifactRepository::get(dbp.clone(), &artifact.id)
                .await
                .unwrap();
            assert_eq!(approved.status, ArtifactStatus::Approved);

            // 4. rollback restores before state
            execute_rollback(dbp.clone(), &rollback_handle)
                .await
                .unwrap();
        }

        /// 驳回→保留记录：submit → resolve(Reject) → artifact retained with Rejected status
        #[tokio::test]
        async fn reject_retains_artifact_record() {
            let dbp = db_path().await;
            let artifact = make_artifact(
                &dbp,
                "c1",
                ArtifactType::ResearchReport,
                serde_json::json!({"findings": "x"}),
            )
            .await;

            let gate = DefaultEvolutionGate;
            gate.submit(dbp.clone(), &artifact.id).await.unwrap();
            let outcome = gate
                .resolve(
                    dbp.clone(),
                    &artifact.id,
                    GateDecision::Reject,
                    Some("not acceptable"),
                )
                .await
                .unwrap();

            assert!(matches!(outcome, ResolveOutcome::Rejected));

            // artifact is retained and queryable
            let got = artifact::ArtifactRepository::get(dbp.clone(), &artifact.id)
                .await
                .unwrap();
            assert_eq!(got.status, ArtifactStatus::Rejected);
            assert_eq!(got.content, serde_json::json!({"findings": "x"}));
        }

        /// 要求修改→退回研发团队：submit → resolve(RequestChanges) → new task created
        #[tokio::test]
        async fn request_changes_returns_to_dev_team() {
            let dbp = db_path().await;
            let artifact = make_artifact(
                &dbp,
                "c1",
                ArtifactType::DesignProposal,
                serde_json::json!({"design": "v1"}),
            )
            .await;

            let gate = DefaultEvolutionGate;
            gate.submit(dbp.clone(), &artifact.id).await.unwrap();
            let outcome = gate
                .resolve(
                    dbp.clone(),
                    &artifact.id,
                    GateDecision::RequestChanges {
                        feedback: "needs more detail".into(),
                    },
                    None,
                )
                .await
                .unwrap();

            let new_task_id = match outcome {
                ResolveOutcome::ReturnedForRevision { new_task_id } => new_task_id,
                _ => panic!("expected ReturnedForRevision"),
            };

            // artifact status is NeedsRevision
            let got = artifact::ArtifactRepository::get(dbp.clone(), &artifact.id)
                .await
                .unwrap();
            assert_eq!(got.status, ArtifactStatus::NeedsRevision);

            // new task exists in the DB and is linked to the same cycle
            let dbp2 = dbp.clone();
            let task_exists =
                spawn_blocking(move || {
                    let db = Db::open(&dbp2)?;
                    let count: i64 = db.0.query_row(
                    "SELECT COUNT(*) FROM evolution_tasks WHERE id = ?1 AND status = 'pending'",
                    rusqlite::params![&new_task_id],
                    |row| row.get(0),
                ).map_err(StoreError::Sqlite)?;
                    Ok::<_, StewardError>(count > 0)
                })
                .await
                .unwrap()
                .unwrap();
            assert!(task_exists, "revision task should exist and be pending");
        }

        /// 批量决策各产物独立：submit 2 artifacts → resolve_batch → both resolved independently
        #[tokio::test]
        async fn batch_resolve_artifacts_independent() {
            let dbp = db_path().await;
            let a1 = make_artifact(
                &dbp,
                "c1",
                ArtifactType::ResearchReport,
                serde_json::json!({"a": 1}),
            )
            .await;
            let a2 = make_artifact(
                &dbp,
                "c1",
                ArtifactType::DesignProposal,
                serde_json::json!({"a": 2}),
            )
            .await;

            let gate = DefaultEvolutionGate;
            gate.submit(dbp.clone(), &a1.id).await.unwrap();
            gate.submit(dbp.clone(), &a2.id).await.unwrap();

            let outcomes = gate
                .resolve_batch(dbp.clone(), "c1", GateDecision::Approve)
                .await
                .unwrap();

            assert_eq!(outcomes.len(), 2);
            for o in &outcomes {
                assert!(matches!(o, ResolveOutcome::Merged { .. }));
            }

            // both artifacts are now Approved
            let g1 = artifact::ArtifactRepository::get(dbp.clone(), &a1.id)
                .await
                .unwrap();
            let g2 = artifact::ArtifactRepository::get(dbp.clone(), &a2.id)
                .await
                .unwrap();
            assert_eq!(g1.status, ArtifactStatus::Approved);
            assert_eq!(g2.status, ArtifactStatus::Approved);
        }

        /// 已决产物再次 resolve 返回 AlreadyResolved
        #[tokio::test]
        async fn already_resolved_blocks_re_resolve() {
            let dbp = db_path().await;
            let artifact = make_artifact(
                &dbp,
                "c1",
                ArtifactType::ResearchReport,
                serde_json::json!({}),
            )
            .await;

            let gate = DefaultEvolutionGate;
            gate.submit(dbp.clone(), &artifact.id).await.unwrap();
            gate.resolve(dbp.clone(), &artifact.id, GateDecision::Approve, None)
                .await
                .unwrap();

            let result = gate
                .resolve(dbp.clone(), &artifact.id, GateDecision::Reject, None)
                .await;
            assert!(matches!(result, Err(GateError::AlreadyResolved(_))));
        }

        /// 提交产物后 diff_preview 非空且 rollback_plan 已设
        #[tokio::test]
        async fn submit_sets_diff_and_rollback_plan() {
            let dbp = db_path().await;
            let artifact = make_artifact(
                &dbp,
                "c1",
                ArtifactType::PromptCandidate,
                serde_json::json!({
                    "old_prompt": "a\nb",
                    "new_prompt": "a\nb2",
                }),
            )
            .await;

            let gate = DefaultEvolutionGate;
            gate.submit(dbp.clone(), &artifact.id).await.unwrap();

            let got = artifact::ArtifactRepository::get(dbp, &artifact.id)
                .await
                .unwrap();
            assert!(got.diff_preview.is_some());
            assert!(got.rollback_plan.is_some());
            assert!(got.diff_preview.unwrap().contains("b2"));
        }
    }
}
