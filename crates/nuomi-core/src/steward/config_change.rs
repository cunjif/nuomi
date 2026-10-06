//! Config change proposal, confirmation, and rollback.
//! (K-Steward-3, T3-1 ~ T3-5)
//!
//! The steward produces config change proposals (Provider/Role/Team/AgentProfile),
//! stores them as `evolution_artifacts` (type=`config_change`, status=`pending_review`),
//! and on confirmation writes the change + a rollback snapshot.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::task::spawn_blocking;

use crate::domain::{
    now_ms, ArtifactStatus, ArtifactType, CyclePhase, CycleStatus, DevRoleKind, EvolutionArtifact,
    EvolutionCycle, EvolutionTask, StewardChangeSnapshot, TaskPhase, TriggerSource,
};
use crate::store::repos::{agent_profiles, providers, roles, steward, teams};
use crate::store::Db;

use super::StewardError;

/// Maximum diff preview length (8 KB).
const DIFF_PREVIEW_MAX_BYTES: usize = 8 * 1024;

/// Identifies which config entity a proposal targets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ConfigTarget {
    Provider { id: String },
    Role { id: String },
    Team { id: String },
    AgentProfile { id: String },
}

impl ConfigTarget {
    pub fn target_type(&self) -> &'static str {
        match self {
            ConfigTarget::Provider { .. } => "provider",
            ConfigTarget::Role { .. } => "role",
            ConfigTarget::Team { .. } => "team",
            ConfigTarget::AgentProfile { .. } => "agent_profile",
        }
    }

    pub fn target_id(&self) -> &str {
        match self {
            ConfigTarget::Provider { id } => id,
            ConfigTarget::Role { id } => id,
            ConfigTarget::Team { id } => id,
            ConfigTarget::AgentProfile { id } => id,
        }
    }
}

/// A config change proposal awaiting user confirmation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangeProposal {
    pub proposal_id: String,
    pub target_type: String,
    pub target_id: String,
    pub before: serde_json::Value,
    pub after: serde_json::Value,
    pub diff_preview: String,
    pub created_at: i64,
}

/// The result of confirming a config change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigChangeResult {
    pub proposal_id: String,
    pub snapshot_id: String,
    pub target_type: String,
    pub target_id: String,
}

/// Reads the current config for a target as JSON.
fn read_current_config(
    conn: &rusqlite::Connection,
    target: &ConfigTarget,
) -> Result<serde_json::Value, StewardError> {
    match target {
        ConfigTarget::Provider { id } => {
            let p =
                providers::get_provider(conn, id).map_err(map_store_not_found("provider", id))?;
            Ok(serde_json::to_value(&p)?)
        }
        ConfigTarget::Role { id } => {
            let r = roles::get(conn, id).map_err(map_store_not_found("role", id))?;
            Ok(serde_json::to_value(&r)?)
        }
        ConfigTarget::Team { id } => {
            let t = teams::get(conn, id).map_err(map_store_not_found("team", id))?;
            Ok(serde_json::to_value(&t)?)
        }
        ConfigTarget::AgentProfile { id } => {
            let a =
                agent_profiles::get(conn, id).map_err(map_store_not_found("agent_profile", id))?;
            Ok(serde_json::to_value(&a)?)
        }
    }
}

fn map_store_not_found<'a>(
    entity: &'static str,
    id: &'a str,
) -> impl Fn(crate::store::StoreError) -> StewardError + 'a {
    let id = id.to_string();
    move |e: crate::store::StoreError| match e {
        crate::store::StoreError::NotFound { .. } => StewardError::NotFound {
            entity,
            id: id.clone(),
        },
        other => StewardError::Store(other.to_string()),
    }
}

/// Generates a text diff between two JSON values, truncated to 8 KB.
fn generate_diff(before: &serde_json::Value, after: &serde_json::Value) -> String {
    let before_str = serde_json::to_string_pretty(before).unwrap_or_default();
    let after_str = serde_json::to_string_pretty(after).unwrap_or_default();
    let mut diff = String::new();
    diff.push_str("--- before\n");
    diff.push_str(&before_str);
    diff.push_str("\n+++ after\n");
    diff.push_str(&after_str);
    if diff.len() > DIFF_PREVIEW_MAX_BYTES {
        diff.truncate(DIFF_PREVIEW_MAX_BYTES);
        diff.push_str("\n... (truncated)");
    }
    diff
}

/// Merges a partial update into a base JSON value (shallow merge for objects).
fn merge_json(base: &serde_json::Value, patch: &serde_json::Value) -> serde_json::Value {
    match (base, patch) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(patch_map)) => {
            let mut result = base_map.clone();
            for (k, v) in patch_map {
                result.insert(k.clone(), v.clone());
            }
            serde_json::Value::Object(result)
        }
        (_, patch) => patch.clone(),
    }
}

/// Produces a config change proposal.
///
/// Reads the current config → merges the `intent` (JSON patch) to produce the
/// proposed `after` → generates a diff preview → stores as an
/// `evolution_artifact` (type=`config_change`, status=`pending_review`).
///
/// The `intent` is a JSON string describing the desired changes. It is merged
/// shallowly into the current config for object types.
pub async fn propose(
    db_path: Arc<str>,
    target: ConfigTarget,
    intent: &str,
) -> Result<ConfigChangeProposal, StewardError> {
    let intent = intent.to_string();
    spawn_blocking(move || {
        let mut db = Db::open(&db_path)?;
        let conn = &mut db.0;
        let tx = conn.transaction()?;

        let before = read_current_config(&tx, &target)?;

        let patch: serde_json::Value = serde_json::from_str(&intent)
            .map_err(|e| StewardError::Store(format!("invalid intent JSON: {e}")))?;
        let after = merge_json(&before, &patch);

        if after.is_null() {
            check_references(&tx, &target)?;
        }

        let diff_preview = generate_diff(&before, &after);

        let now = now_ms();
        let cycle_id = crate::domain::new_id();
        let cycle = EvolutionCycle {
            id: cycle_id.clone(),
            trigger_source: TriggerSource::User,
            trigger_context: "config_change".into(),
            phase: CyclePhase::Gate,
            status: CycleStatus::Running,
            created_at: now,
            ended_at: None,
        };
        steward::insert_cycle(&tx, &cycle)?;

        let task_id = crate::domain::new_id();
        let task = EvolutionTask {
            id: task_id.clone(),
            cycle_id: cycle_id.clone(),
            phase: TaskPhase::Develop,
            dev_role: DevRoleKind::Developer,
            depends_on: vec![],
            status: crate::domain::StewardTaskStatus::Completed,
            acceptance_criteria: "config change proposal".into(),
            trigger_source: "steward".into(),
            created_at: now,
            updated_at: now,
        };
        steward::insert_task(&tx, &task)?;

        let proposal_id = crate::domain::new_id();
        let artifact = EvolutionArtifact {
            id: proposal_id.clone(),
            task_id: task_id.clone(),
            produced_by_role: "steward".into(),
            artifact_type: ArtifactType::ConfigChange,
            content: serde_json::json!({
                "target_type": target.target_type(),
                "target_id": target.target_id(),
                "before": before,
                "after": after,
                "intent": intent,
            }),
            status: ArtifactStatus::PendingReview,
            diff_preview: Some(diff_preview.clone()),
            rollback_plan: None,
            created_at: now,
        };
        steward::insert_artifact(&tx, &artifact)?;
        tx.commit()?;

        Ok(ConfigChangeProposal {
            proposal_id,
            target_type: target.target_type().into(),
            target_id: target.target_id().into(),
            before,
            after,
            diff_preview,
            created_at: now,
        })
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

/// T8-4: wraps `propose` with `steward.config_proposed` event.
pub async fn propose_with_event(
    db_path: Arc<str>,
    target: ConfigTarget,
    intent: &str,
) -> Result<ConfigChangeProposal, StewardError> {
    let result = propose(db_path.clone(), target, intent).await?;
    super::events::publish_event(
        db_path,
        &result.proposal_id,
        super::events::StewardEventKind::ConfigProposed,
        &serde_json::json!({
            "proposal_id": result.proposal_id,
            "target": format!("{}#{}", result.target_type, result.target_id),
            "diff": result.diff_preview,
        }),
    )
    .await;
    Ok(result)
}

/// Checks if the target is referenced by other entities (for delete operations).
/// Returns `Ok(())` if safe, or `Err(StewardError::Conflict)` with reference details.
pub fn check_references(
    conn: &rusqlite::Connection,
    target: &ConfigTarget,
) -> Result<(), StewardError> {
    match target {
        ConfigTarget::Provider { id } => {
            let refs = crate::services::reference_pre_check::check_provider_refs(conn, id)?;
            if !refs.is_empty() {
                return Err(StewardError::Conflict {
                    entity: "provider",
                    id: id.clone(),
                    reason: format!(
                        "referenced by roles: {:?}, teams: {:?}, sessions: {:?}",
                        refs.roles, refs.teams, refs.sessions
                    ),
                });
            }
        }
        ConfigTarget::Role { id } => {
            let refs = crate::services::reference_pre_check::check_role_refs(conn, id)?;
            if !refs.is_empty() {
                return Err(StewardError::Conflict {
                    entity: "role",
                    id: id.clone(),
                    reason: format!(
                        "referenced by teams: {:?}, sessions: {:?}",
                        refs.teams, refs.sessions
                    ),
                });
            }
        }
        _ => {}
    }
    Ok(())
}

/// Confirms a config change proposal: validates the proposal exists and is
/// pending → checks baseline (before matches current) → writes the config →
/// stores a rollback snapshot → marks artifact as approved.
pub async fn confirm_config_change(
    db_path: Arc<str>,
    proposal_id: &str,
) -> Result<ConfigChangeResult, StewardError> {
    let proposal_id = proposal_id.to_string();
    spawn_blocking(move || {
        let mut db = Db::open(&db_path)?;
        let conn = &mut db.0;
        let tx = conn.transaction()?;

        let artifact = steward::get_artifact(&tx, &proposal_id)?;
        if artifact.artifact_type != ArtifactType::ConfigChange {
            return Err(StewardError::NotFound {
                entity: "config_change_proposal",
                id: proposal_id.clone(),
            });
        }
        if artifact.status != ArtifactStatus::PendingReview {
            return Err(StewardError::Conflict {
                entity: "config_change_proposal",
                id: proposal_id.clone(),
                reason: format!("proposal is already {:?}", artifact.status),
            });
        }

        let content = &artifact.content;
        let target_type = content["target_type"]
            .as_str()
            .ok_or_else(|| StewardError::Store("missing target_type in artifact".into()))?;
        let target_id = content["target_id"]
            .as_str()
            .ok_or_else(|| StewardError::Store("missing target_id in artifact".into()))?;
        let before = &content["before"];
        let after = &content["after"];

        let current = read_current_config(&tx, &parse_target(target_type, target_id)?)?;
        if before != &current {
            return Err(StewardError::Conflict {
                entity: "config_change_proposal",
                id: proposal_id.clone(),
                reason: "baseline drift: current config differs from proposal's before".into(),
            });
        }

        write_config(&tx, target_type, target_id, after)?;

        let snapshot_id = crate::domain::new_id();
        let snapshot = StewardChangeSnapshot {
            id: snapshot_id.clone(),
            proposal_id: proposal_id.clone(),
            target_type: target_type.into(),
            target_id: target_id.into(),
            before: before.clone(),
            after: after.clone(),
            created_at: now_ms(),
        };
        steward::insert_change_snapshot(&tx, &snapshot)?;

        steward::update_artifact_status(&tx, &proposal_id, ArtifactStatus::Approved)?;
        tx.commit()?;

        Ok(ConfigChangeResult {
            proposal_id,
            snapshot_id,
            target_type: target_type.into(),
            target_id: target_id.into(),
        })
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

/// Rolls back a config change by restoring the `before` state from the snapshot.
pub async fn rollback_config_change(
    db_path: Arc<str>,
    snapshot_id: &str,
) -> Result<(), StewardError> {
    let snapshot_id = snapshot_id.to_string();
    spawn_blocking(move || {
        let mut db = Db::open(&db_path)?;
        let conn = &mut db.0;
        let tx = conn.transaction()?;

        let snapshot = steward::get_change_snapshot(&tx, &snapshot_id).map_err(|e| match e {
            crate::store::StoreError::NotFound { .. } => StewardError::NotFound {
                entity: "change_snapshot",
                id: snapshot_id.clone(),
            },
            other => StewardError::Store(other.to_string()),
        })?;

        write_config(
            &tx,
            &snapshot.target_type,
            &snapshot.target_id,
            &snapshot.before,
        )?;

        tx.commit()?;
        Ok(())
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

/// T8-4: wraps `confirm_config_change` with `steward.config_confirmed` event.
pub async fn confirm_config_change_with_event(
    db_path: Arc<str>,
    proposal_id: &str,
) -> Result<ConfigChangeResult, StewardError> {
    let result = confirm_config_change(db_path.clone(), proposal_id).await?;
    super::events::publish_event(
        db_path,
        &result.proposal_id,
        super::events::StewardEventKind::ConfigConfirmed,
        &serde_json::json!({
            "proposal_id": result.proposal_id,
            "snapshot_id": result.snapshot_id,
        }),
    )
    .await;
    Ok(result)
}

/// T8-4: wraps `rollback_config_change` with `steward.config_rolled_back` event.
pub async fn rollback_config_change_with_event(
    db_path: Arc<str>,
    snapshot_id: &str,
) -> Result<(), StewardError> {
    rollback_config_change(db_path.clone(), snapshot_id).await?;
    super::events::publish_event(
        db_path,
        snapshot_id,
        super::events::StewardEventKind::ConfigRolledBack,
        &serde_json::json!({"snapshot_id": snapshot_id}),
    )
    .await;
    Ok(())
}

fn parse_target(target_type: &str, target_id: &str) -> Result<ConfigTarget, StewardError> {
    match target_type {
        "provider" => Ok(ConfigTarget::Provider {
            id: target_id.into(),
        }),
        "role" => Ok(ConfigTarget::Role {
            id: target_id.into(),
        }),
        "team" => Ok(ConfigTarget::Team {
            id: target_id.into(),
        }),
        "agent_profile" => Ok(ConfigTarget::AgentProfile {
            id: target_id.into(),
        }),
        _ => Err(StewardError::Store(format!(
            "unknown target_type: {target_type}"
        ))),
    }
}

/// Writes a config value to the appropriate table.
fn write_config(
    conn: &rusqlite::Connection,
    target_type: &str,
    _target_id: &str,
    value: &serde_json::Value,
) -> Result<(), StewardError> {
    match target_type {
        "provider" => {
            let p: crate::domain::ProviderConfig = serde_json::from_value(value.clone())
                .map_err(|e| StewardError::Store(format!("invalid provider config: {e}")))?;
            providers::update_provider(conn, &p)?;
        }
        "role" => {
            let r: crate::domain::Role = serde_json::from_value(value.clone())
                .map_err(|e| StewardError::Store(format!("invalid role config: {e}")))?;
            roles::update(conn, &r)?;
        }
        "team" => {
            let t: crate::domain::Team = serde_json::from_value(value.clone())
                .map_err(|e| StewardError::Store(format!("invalid team config: {e}")))?;
            teams::update(conn, &t)?;
        }
        "agent_profile" => {
            let a: crate::domain::AgentProfile = serde_json::from_value(value.clone())
                .map_err(|e| StewardError::Store(format!("invalid agent_profile config: {e}")))?;
            agent_profiles::update(conn, &a)?;
        }
        _ => {
            return Err(StewardError::Store(format!(
                "unknown target_type: {target_type}"
            )))
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Role;
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

    fn seed_role(conn: &rusqlite::Connection, id: &str, name: &str) {
        let now = now_ms();
        let role = Role {
            id: id.into(),
            name: name.into(),
            provider_id: None,
            provider_ids: vec![],
            system_prompt_override: None,
            tool_allowlist: vec![],
            required_capabilities: vec![],
            temperature: Some(0.7),
            max_tokens: None,
            params: serde_json::json!({}),
            builtin: false,
            generated: false,
            ephemeral: false,
            source: None,
            created_at: now,
            updated_at: now,
        };
        roles::insert(conn, &role).unwrap();
    }

    #[tokio::test]
    async fn propose_confirm_rollback_full_flow() {
        let dbp = db_path().await;
        let role_id = "role_test_1".to_string();
        {
            let db = Db::open(&dbp).unwrap();
            seed_role(&db.0, &role_id, "Test Role");
        }

        let target = ConfigTarget::Role {
            id: role_id.clone(),
        };
        let intent = serde_json::json!({ "temperature": 0.5 }).to_string();
        let proposal = propose(dbp.clone(), target, &intent).await.unwrap();
        assert_eq!(proposal.target_type, "role");
        assert_eq!(proposal.target_id, role_id);
        assert!(proposal.diff_preview.contains("0.7"));
        assert!(proposal.diff_preview.contains("0.5"));
        assert!(proposal.diff_preview.len() <= DIFF_PREVIEW_MAX_BYTES + 20);

        let result = confirm_config_change(dbp.clone(), &proposal.proposal_id)
            .await
            .unwrap();
        assert_eq!(result.target_type, "role");
        assert_eq!(result.target_id, role_id);

        {
            let db = Db::open(&dbp).unwrap();
            let role = roles::get(&db.0, &role_id).unwrap();
            assert_eq!(role.temperature, Some(0.5));
        }

        rollback_config_change(dbp.clone(), &result.snapshot_id)
            .await
            .unwrap();
        {
            let db = Db::open(&dbp).unwrap();
            let role = roles::get(&db.0, &role_id).unwrap();
            assert_eq!(role.temperature, Some(0.7));
        }
    }

    #[tokio::test]
    async fn baseline_drift_rejects_confirm() {
        let dbp = db_path().await;
        let role_id = "role_drift".to_string();
        {
            let db = Db::open(&dbp).unwrap();
            seed_role(&db.0, &role_id, "Drift Role");
        }

        let target = ConfigTarget::Role {
            id: role_id.clone(),
        };
        let intent = serde_json::json!({ "temperature": 0.3 }).to_string();
        let proposal = propose(dbp.clone(), target, &intent).await.unwrap();

        {
            let db = Db::open(&dbp).unwrap();
            let mut role = roles::get(&db.0, &role_id).unwrap();
            role.temperature = Some(0.9);
            roles::update(&db.0, &role).unwrap();
        }

        let err = confirm_config_change(dbp, &proposal.proposal_id)
            .await
            .unwrap_err();
        assert!(matches!(err, StewardError::Conflict { .. }));
    }

    #[tokio::test]
    async fn delete_referenced_role_rejected() {
        let dbp = db_path().await;
        let role_id = "role_refed".to_string();
        {
            let db = Db::open(&dbp).unwrap();
            seed_role(&db.0, &role_id, "Referenced Role");
            let now = now_ms();
            let team = crate::domain::Team {
                id: "team_1".into(),
                name: "Team 1".into(),
                topology: crate::domain::TeamTopology::Pipeline,
                member_role_ids: vec![role_id.clone()],
                config: serde_json::json!({}),
                created_at: now,
                updated_at: now,
            };
            teams::insert(&db.0, &team).unwrap();
        }

        let target = ConfigTarget::Role {
            id: role_id.clone(),
        };
        let intent = serde_json::json!(null).to_string();
        let err = propose(dbp, target, &intent).await.unwrap_err();
        assert!(matches!(err, StewardError::Conflict { .. }));
    }

    #[tokio::test]
    async fn propose_nonexistent_target_returns_not_found() {
        let dbp = db_path().await;
        let target = ConfigTarget::Role {
            id: "no_such_role".into(),
        };
        let err = propose(dbp, target, "{}").await.unwrap_err();
        assert!(matches!(err, StewardError::NotFound { .. }));
    }

    #[tokio::test]
    async fn confirm_already_confirmed_returns_conflict() {
        let dbp = db_path().await;
        let role_id = "role_double".to_string();
        {
            let db = Db::open(&dbp).unwrap();
            seed_role(&db.0, &role_id, "Double Confirm");
        }

        let target = ConfigTarget::Role {
            id: role_id.clone(),
        };
        let proposal = propose(dbp.clone(), target, r#"{"temperature":0.1}"#)
            .await
            .unwrap();
        confirm_config_change(dbp.clone(), &proposal.proposal_id)
            .await
            .unwrap();
        let err = confirm_config_change(dbp, &proposal.proposal_id)
            .await
            .unwrap_err();
        assert!(matches!(err, StewardError::Conflict { .. }));
    }

    #[tokio::test]
    async fn propose_with_event_creates_proposal_and_event() {
        let dbp = db_path().await;
        let role_id = "role_we1".to_string();
        {
            let db = Db::open(&dbp).unwrap();
            seed_role(&db.0, &role_id, "Event Role");
        }
        let target = ConfigTarget::Role { id: role_id };
        let proposal = propose_with_event(dbp, target, r#"{"temperature":0.2}"#)
            .await
            .unwrap();
        assert!(!proposal.proposal_id.is_empty());
    }

    #[tokio::test]
    async fn rollback_config_change_restores_original() {
        let dbp = db_path().await;
        let role_id = "role_rb1".to_string();
        {
            let db = Db::open(&dbp).unwrap();
            seed_role(&db.0, &role_id, "Rollback Role");
        }
        let target = ConfigTarget::Role {
            id: role_id.clone(),
        };
        let proposal = propose(dbp.clone(), target, r#"{"temperature":0.3}"#)
            .await
            .unwrap();
        let result = confirm_config_change(dbp.clone(), &proposal.proposal_id)
            .await
            .unwrap();
        rollback_config_change(dbp, &result.snapshot_id)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn confirm_with_event_confirms_and_publishes() {
        let dbp = db_path().await;
        let role_id = "role_cwe1".to_string();
        {
            let db = Db::open(&dbp).unwrap();
            seed_role(&db.0, &role_id, "Confirm Event");
        }
        let target = ConfigTarget::Role {
            id: role_id.clone(),
        };
        let proposal = propose(dbp.clone(), target, r#"{"temperature":0.4}"#)
            .await
            .unwrap();
        let result = confirm_config_change_with_event(dbp, &proposal.proposal_id)
            .await
            .unwrap();
        assert!(!result.snapshot_id.is_empty());
    }

    #[tokio::test]
    async fn rollback_with_event_rolls_back_and_publishes() {
        let dbp = db_path().await;
        let role_id = "role_rwe1".to_string();
        {
            let db = Db::open(&dbp).unwrap();
            seed_role(&db.0, &role_id, "Rollback Event");
        }
        let target = ConfigTarget::Role {
            id: role_id.clone(),
        };
        let proposal = propose(dbp.clone(), target, r#"{"temperature":0.5}"#)
            .await
            .unwrap();
        let result = confirm_config_change(dbp.clone(), &proposal.proposal_id)
            .await
            .unwrap();
        rollback_config_change_with_event(dbp, &result.snapshot_id)
            .await
            .unwrap();
    }
}
