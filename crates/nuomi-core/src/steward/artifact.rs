//! Artifact repository + diff preview generator.
//! (K-Steward-7, T7-1 ~ T7-2)
//!
//! `ArtifactRepository` is a thin async wrapper around the synchronous
//! `store::repos::steward` artifact CRUD. The diff preview generator
//! produces a human-readable diff per `ArtifactType`, truncated to 8KB.

use std::sync::Arc;

use tokio::task::spawn_blocking;

use crate::domain::{ArtifactStatus, ArtifactType, EvolutionArtifact};
use crate::store::repos::steward;
use crate::store::Db;

use super::StewardError;

/// Maximum diff preview length (spec §5.7.1 规则 3).
const DIFF_MAX_BYTES: usize = 8 * 1024;

/// Async wrapper around the steward artifact store operations.
pub struct ArtifactRepository;

impl ArtifactRepository {
    pub async fn insert(
        db_path: Arc<str>,
        artifact: &EvolutionArtifact,
    ) -> Result<(), StewardError> {
        let artifact = artifact.clone();
        spawn_blocking(move || {
            let db = Db::open(&db_path)?;
            steward::insert_artifact(&db.0, &artifact)?;
            Ok::<_, StewardError>(())
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))?
    }

    pub async fn get(db_path: Arc<str>, id: &str) -> Result<EvolutionArtifact, StewardError> {
        let id = id.to_string();
        spawn_blocking(move || {
            let db = Db::open(&db_path)?;
            Ok::<_, StewardError>(steward::get_artifact(&db.0, &id)?)
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))?
    }

    pub async fn list_by_cycle(
        db_path: Arc<str>,
        cycle_id: &str,
    ) -> Result<Vec<EvolutionArtifact>, StewardError> {
        let cycle_id = cycle_id.to_string();
        spawn_blocking(move || {
            let db = Db::open(&db_path)?;
            Ok::<_, StewardError>(steward::list_artifacts_by_cycle(&db.0, &cycle_id)?)
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))?
    }

    pub async fn list_by_status(
        db_path: Arc<str>,
        status: ArtifactStatus,
    ) -> Result<Vec<EvolutionArtifact>, StewardError> {
        spawn_blocking(move || {
            let db = Db::open(&db_path)?;
            Ok::<_, StewardError>(steward::list_artifacts_by_status(&db.0, status, 100)?)
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))?
    }

    pub async fn update_status(
        db_path: Arc<str>,
        id: &str,
        status: ArtifactStatus,
    ) -> Result<(), StewardError> {
        let id = id.to_string();
        spawn_blocking(move || {
            let db = Db::open(&db_path)?;
            steward::update_artifact_status(&db.0, &id, status)?;
            Ok::<_, StewardError>(())
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))?
    }

    pub async fn set_diff_rollback(
        db_path: Arc<str>,
        id: &str,
        diff_preview: Option<&str>,
        rollback_plan: Option<&serde_json::Value>,
    ) -> Result<(), StewardError> {
        let id = id.to_string();
        let diff_preview = diff_preview.map(String::from);
        let rollback_plan = rollback_plan.cloned();
        spawn_blocking(move || {
            let db = Db::open(&db_path)?;
            steward::set_artifact_diff_rollback(
                &db.0,
                &id,
                diff_preview.as_deref(),
                rollback_plan.as_ref(),
            )?;
            Ok::<_, StewardError>(())
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))?
    }
}

// ============================================================ diff preview generator

/// Generates a human-readable diff preview for an artifact, truncated to 8KB.
///
/// Dispatches by `ArtifactType`:
/// - `PromptCandidate`: old vs new prompt text diff
/// - `ConfigChange`: config JSON diff + affected Role/Team list
/// - `NewRole` / `NewTeam`: full content preview
/// - Others: summary
pub fn generate_diff_preview(artifact: &EvolutionArtifact) -> String {
    let raw = match artifact.artifact_type {
        ArtifactType::PromptCandidate => diff_prompt_candidate(&artifact.content),
        ArtifactType::ConfigChange => diff_config_change(&artifact.content),
        ArtifactType::NewRole | ArtifactType::NewTeam => {
            serde_json::to_string_pretty(&artifact.content).unwrap_or_default()
        }
        ArtifactType::ResearchReport => summarize_content(&artifact.content, "调研报告"),
        ArtifactType::DesignProposal => summarize_content(&artifact.content, "设计方案"),
        ArtifactType::TestReport => summarize_content(&artifact.content, "测试报告"),
        ArtifactType::Verification => summarize_content(&artifact.content, "验收结论"),
    };
    truncate_to_bytes(raw, DIFF_MAX_BYTES)
}

/// Produces a line-by-line diff between old and new prompt text.
fn diff_prompt_candidate(content: &serde_json::Value) -> String {
    let old = content
        .get("old_prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let new = content
        .get("new_prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    line_diff(old, new)
}

/// Produces a JSON key-by-key diff for config changes.
fn diff_config_change(content: &serde_json::Value) -> String {
    let before = content
        .get("before")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let after = content
        .get("after")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let mut parts = Vec::new();
    parts.push("=== 配置变更 diff ===".to_string());
    parts.push(format!(
        "--- before\n{}",
        serde_json::to_string_pretty(&before).unwrap_or_default()
    ));
    parts.push(format!(
        "+++ after\n{}",
        serde_json::to_string_pretty(&after).unwrap_or_default()
    ));
    if let Some(affected) = content.get("affected_roles").and_then(|v| v.as_array()) {
        parts.push(format!("影响 Role: {}", affected.len()));
    }
    if let Some(affected) = content.get("affected_teams").and_then(|v| v.as_array()) {
        parts.push(format!("影响 Team: {}", affected.len()));
    }
    parts.join("\n")
}

/// Produces a simple summary for non-structured artifact types.
fn summarize_content(content: &serde_json::Value, label: &str) -> String {
    let keys: Vec<String> = content
        .as_object()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    format!("[{label}] 字段: {}", keys.join(", "))
}

/// A minimal line-by-line diff (added/removed markers).
fn line_diff(old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let mut result = Vec::new();
    let max = old_lines.len().max(new_lines.len());
    for i in 0..max {
        match (old_lines.get(i), new_lines.get(i)) {
            (Some(o), Some(n)) if o == n => result.push(format!("  {n}")),
            (Some(o), Some(n)) => {
                result.push(format!("- {o}"));
                result.push(format!("+ {n}"));
            }
            (Some(o), None) => result.push(format!("- {o}")),
            (None, Some(n)) => result.push(format!("+ {n}")),
            (None, None) => {}
        }
    }
    result.join("\n")
}

/// Truncates a string to at most `max_bytes` bytes, appending "..." if truncated.
fn truncate_to_bytes(s: String, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut truncated = s[..end].to_string();
    truncated.push_str("\n... (truncated)");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        now_ms, CyclePhase, CycleStatus, DevRoleKind, EvolutionCycle, EvolutionTask,
        StewardTaskStatus, TaskPhase, TriggerSource,
    };
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

    async fn seed_cycle_and_task(dbp: &Arc<str>) -> String {
        let cycle = EvolutionCycle {
            id: crate::domain::new_id(),
            trigger_source: TriggerSource::User,
            trigger_context: "test".into(),
            phase: CyclePhase::Develop,
            status: CycleStatus::Running,
            created_at: now_ms(),
            ended_at: None,
        };
        let cycle_id = cycle.id.clone();
        let task = EvolutionTask {
            id: crate::domain::new_id(),
            cycle_id: cycle_id.clone(),
            phase: TaskPhase::Develop,
            dev_role: DevRoleKind::Developer,
            depends_on: vec![],
            status: StewardTaskStatus::Pending,
            acceptance_criteria: "test".into(),
            trigger_source: "test".into(),
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let task_id = task.id.clone();
        let dbp1 = dbp.clone();
        spawn_blocking(move || {
            let mut db = Db::open(&dbp1)?;
            steward::insert_cycle(&mut db.0, &cycle)?;
            steward::insert_task(&mut db.0, &task)?;
            Ok::<_, StewardError>(())
        })
        .await
        .unwrap()
        .unwrap();
        task_id
    }

    fn make_artifact(
        task_id: &str,
        artifact_type: ArtifactType,
        content: serde_json::Value,
    ) -> EvolutionArtifact {
        EvolutionArtifact {
            id: crate::domain::new_id(),
            task_id: task_id.into(),
            produced_by_role: "steward".into(),
            artifact_type,
            content,
            status: ArtifactStatus::PendingReview,
            diff_preview: None,
            rollback_plan: None,
            created_at: now_ms(),
        }
    }

    #[tokio::test]
    async fn repository_insert_get_roundtrip() {
        let dbp = db_path().await;
        let task_id = seed_cycle_and_task(&dbp).await;
        let artifact = make_artifact(
            &task_id,
            ArtifactType::ResearchReport,
            serde_json::json!({"x": 1}),
        );
        let id = artifact.id.clone();
        ArtifactRepository::insert(dbp.clone(), &artifact)
            .await
            .unwrap();
        let got = ArtifactRepository::get(dbp, &id).await.unwrap();
        assert_eq!(got.id, id);
        assert_eq!(got.content, serde_json::json!({"x": 1}));
    }

    #[tokio::test]
    async fn repository_update_status() {
        let dbp = db_path().await;
        let task_id = seed_cycle_and_task(&dbp).await;
        let artifact = make_artifact(
            &task_id,
            ArtifactType::ResearchReport,
            serde_json::json!({}),
        );
        let id = artifact.id.clone();
        ArtifactRepository::insert(dbp.clone(), &artifact)
            .await
            .unwrap();
        ArtifactRepository::update_status(dbp.clone(), &id, ArtifactStatus::Approved)
            .await
            .unwrap();
        let got = ArtifactRepository::get(dbp, &id).await.unwrap();
        assert_eq!(got.status, ArtifactStatus::Approved);
    }

    #[tokio::test]
    async fn repository_set_diff_rollback() {
        let dbp = db_path().await;
        let task_id = seed_cycle_and_task(&dbp).await;
        let artifact = make_artifact(
            &task_id,
            ArtifactType::PromptCandidate,
            serde_json::json!({}),
        );
        let id = artifact.id.clone();
        ArtifactRepository::insert(dbp.clone(), &artifact)
            .await
            .unwrap();
        let rollback = serde_json::json!({"before_ref": "v1"});
        ArtifactRepository::set_diff_rollback(dbp.clone(), &id, Some("diff text"), Some(&rollback))
            .await
            .unwrap();
        let got = ArtifactRepository::get(dbp, &id).await.unwrap();
        assert_eq!(got.diff_preview.as_deref(), Some("diff text"));
        assert!(got.rollback_plan.is_some());
    }

    #[test]
    fn diff_prompt_candidate_shows_changes() {
        let artifact = make_artifact(
            "t1",
            ArtifactType::PromptCandidate,
            serde_json::json!({
                "old_prompt": "line1\nline2",
                "new_prompt": "line1\nline2_changed",
            }),
        );
        let diff = generate_diff_preview(&artifact);
        assert!(diff.contains("- line2"));
        assert!(diff.contains("+ line2_changed"));
        assert!(diff.contains("  line1"));
    }

    #[test]
    fn diff_config_change_shows_before_after() {
        let artifact = make_artifact(
            "t1",
            ArtifactType::ConfigChange,
            serde_json::json!({
                "before": {"temperature": 0.7},
                "after": {"temperature": 0.9},
            }),
        );
        let diff = generate_diff_preview(&artifact);
        assert!(diff.contains("before"));
        assert!(diff.contains("after"));
        assert!(diff.contains("0.7"));
        assert!(diff.contains("0.9"));
    }

    #[test]
    fn diff_truncates_to_8kb() {
        let long_text = "x".repeat(DIFF_MAX_BYTES + 1000);
        let artifact = make_artifact(
            "t1",
            ArtifactType::NewRole,
            serde_json::json!({"data": long_text}),
        );
        let diff = generate_diff_preview(&artifact);
        assert!(
            diff.len() <= DIFF_MAX_BYTES + 20,
            "diff should be truncated near 8KB"
        );
        assert!(diff.ends_with("... (truncated)"));
    }

    #[test]
    fn diff_summary_for_research_report() {
        let artifact = make_artifact(
            "t1",
            ArtifactType::ResearchReport,
            serde_json::json!({"findings": "x", "recommendations": "y"}),
        );
        let diff = generate_diff_preview(&artifact);
        assert!(diff.contains("调研报告"));
        assert!(diff.contains("findings"));
    }
}
