//! Dev team singleton + role bindings. (K-Steward-4, T4-1 ~ T4-3)
//!
//! The dev team is a singleton (1:1 with StewardAi) with 5 role bindings
//! (researcher/designer/developer/tester/verifier). Bindings may be replaced
//! but never deleted.

use std::sync::Arc;

use tokio::task::spawn_blocking;

use crate::domain::{
    now_ms, AgentRefKind, ArtifactStatus, ArtifactType, DevRoleBinding, DevRoleKind, DevTeam,
    EvolutionArtifact, EvolutionTask, StewardAi, StewardTaskStatus, Team, TeamTopology,
};
use crate::harness::EventBus;
use crate::providers::SecretStore;
use crate::services::team_runner;
use crate::store::repos::{sessions, steward, teams};
use crate::store::Db;

use super::StewardError;

/// All 5 dev role kinds in canonical order.
const ALL_ROLE_KINDS: [DevRoleKind; 5] = [
    DevRoleKind::Researcher,
    DevRoleKind::Designer,
    DevRoleKind::Developer,
    DevRoleKind::Tester,
    DevRoleKind::Verifier,
];

/// Ensures the dev team singleton exists: creates steward_ai if missing,
/// then dev_team, then 5 empty role bindings. Idempotent.
pub async fn ensure_dev_team_singleton(db_path: Arc<str>) -> Result<DevTeam, StewardError> {
    spawn_blocking(move || {
        let mut db = Db::open(&db_path)?;
        let conn = &mut db.0;
        let tx = conn.transaction()?;

        let steward_id = match steward::get_steward_ai(&tx)? {
            Some(ai) => ai.id,
            None => {
                let id = crate::domain::new_id();
                let ai = StewardAi {
                    id: id.clone(),
                    ready: false,
                    online_authorized: false,
                    created_at: now_ms(),
                };
                steward::upsert_steward_ai(&tx, &ai)?;
                id
            }
        };

        let team = match steward::get_dev_team(&tx)? {
            Some(t) => t,
            None => {
                let t = DevTeam {
                    id: "steward_dev_team".into(),
                    steward_id: steward_id.clone(),
                    created_at: now_ms(),
                };
                steward::upsert_dev_team(&tx, &t)?;
                t
            }
        };

        let existing = steward::get_dev_role_bindings_all(&tx)?;
        for kind in &ALL_ROLE_KINDS {
            let already = existing.iter().any(|b| b.role_kind == *kind);
            if !already {
                let binding = DevRoleBinding {
                    role_kind: *kind,
                    agent_kind: AgentRefKind::Role,
                    agent_ref_id: String::new(),
                    updated_at: now_ms(),
                };
                steward::upsert_dev_role_binding(&tx, &binding)?;
            }
        }

        tx.commit()?;
        Ok(team)
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

/// Replaces a dev role binding. Does not affect in-flight tasks (they finish
/// with the old binding).
pub async fn set_dev_role_binding(
    db_path: Arc<str>,
    _role_kind: DevRoleKind,
    binding: DevRoleBinding,
) -> Result<(), StewardError> {
    spawn_blocking(move || {
        let mut db = Db::open(&db_path)?;
        let conn = &mut db.0;
        let tx = conn.transaction()?;
        ensure_dev_team_singleton_inner(&tx)?;
        steward::upsert_dev_role_binding(&tx, &binding)?;
        tx.commit()?;
        Ok(())
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

/// T8-7: wraps `set_dev_role_binding` with `steward.dev_role_binding_changed` event.
pub async fn set_dev_role_binding_with_event(
    db_path: Arc<str>,
    role_kind: DevRoleKind,
    binding: DevRoleBinding,
) -> Result<(), StewardError> {
    set_dev_role_binding(db_path.clone(), role_kind, binding.clone()).await?;
    super::events::publish_event(
        db_path,
        &binding.agent_ref_id,
        super::events::StewardEventKind::DevRoleBindingChanged,
        &serde_json::json!({
            "role_kind": role_kind.as_str(),
            "agent_kind": format!("{:?}", binding.agent_kind),
            "agent_ref_id": binding.agent_ref_id,
        }),
    )
    .await;
    Ok(())
}

/// Gets the dev team + all 5 role bindings.
pub async fn get_dev_team(db_path: Arc<str>) -> Result<DevTeamWithBindings, StewardError> {
    spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        let conn = &db.0;
        let team = steward::get_dev_team(conn)?.ok_or(StewardError::NotFound {
            entity: "dev_team",
            id: "steward_dev_team".into(),
        })?;
        let bindings = steward::get_dev_role_bindings_all(conn)?;
        Ok(DevTeamWithBindings { team, bindings })
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

/// Dev team with its role bindings.
#[derive(Debug, Clone)]
pub struct DevTeamWithBindings {
    pub team: DevTeam,
    pub bindings: Vec<DevRoleBinding>,
}

/// Checks if all 5 bindings are non-empty (agent_ref_id is set).
pub fn bindings_ready(bindings: &[DevRoleBinding]) -> bool {
    ALL_ROLE_KINDS.iter().all(|kind| {
        bindings
            .iter()
            .any(|b| b.role_kind == *kind && !b.agent_ref_id.is_empty())
    })
}

fn ensure_dev_team_singleton_inner(tx: &rusqlite::Connection) -> Result<DevTeam, StewardError> {
    let steward_id = match steward::get_steward_ai(tx)? {
        Some(ai) => ai.id,
        None => {
            let id = crate::domain::new_id();
            let ai = StewardAi {
                id: id.clone(),
                ready: false,
                online_authorized: false,
                created_at: now_ms(),
            };
            steward::upsert_steward_ai(tx, &ai)?;
            id
        }
    };
    let team = match steward::get_dev_team(tx)? {
        Some(t) => t,
        None => {
            let t = DevTeam {
                id: "steward_dev_team".into(),
                steward_id: steward_id.clone(),
                created_at: now_ms(),
            };
            steward::upsert_dev_team(tx, &t)?;
            t
        }
    };
    let existing = steward::get_dev_role_bindings_all(tx)?;
    for kind in &ALL_ROLE_KINDS {
        let already = existing.iter().any(|b| b.role_kind == *kind);
        if !already {
            let binding = DevRoleBinding {
                role_kind: *kind,
                agent_kind: AgentRefKind::Role,
                agent_ref_id: String::new(),
                updated_at: now_ms(),
            };
            steward::upsert_dev_role_binding(tx, &binding)?;
        }
    }
    Ok(team)
}

// ============================================================ DevTeamRunner

/// Runs evolution tasks by dispatching them to the bound dev role agents.
pub struct DevTeamRunner;

impl DevTeamRunner {
    /// Dispatches a single evolution task to its bound dev role.
    ///
    /// Looks up the binding → validates → creates a temp team + session →
    /// calls `team_runner::run_team` → stores output as artifact → updates
    /// task status.
    pub async fn run_task(
        db_path: Arc<str>,
        secrets: Arc<dyn SecretStore>,
        task: EvolutionTask,
        bus: Option<EventBus>,
    ) -> Result<EvolutionArtifact, StewardError> {
        let dev_role = task.dev_role;
        let binding = {
            let dp = db_path.clone();
            spawn_blocking(move || {
                let db = Db::open(&dp)?;
                let conn = &db.0;
                let bindings = steward::get_dev_role_bindings_all(conn)?;
                Ok::<_, StewardError>(bindings.into_iter().find(|b| b.role_kind == dev_role))
            })
            .await
            .map_err(|e| StewardError::Store(format!("join error: {e}")))?
        };

        let binding = binding?.ok_or(StewardError::DevTeamNotReady(format!(
            "no binding for role {dev_role:?}"
        )))?;
        if binding.agent_ref_id.is_empty() {
            return Err(StewardError::DevTeamNotReady(format!(
                "role {dev_role:?} binding is empty"
            )));
        }

        let team_id = ensure_temp_team(&db_path, &binding.agent_ref_id).await?;
        let session_id = create_temp_session(&db_path).await?;

        let task_text = format!(
            "[{}/5 {}] {}\n验收标准: {}",
            match task.dev_role {
                DevRoleKind::Researcher => 1,
                DevRoleKind::Designer => 2,
                DevRoleKind::Developer => 3,
                DevRoleKind::Tester => 4,
                DevRoleKind::Verifier => 5,
            },
            task.dev_role.as_str(),
            task.acceptance_criteria,
            task.acceptance_criteria,
        );

        let outcome = team_runner::run_team(
            db_path.clone(),
            bus,
            &team_id,
            &session_id,
            &task_text,
            secrets,
            None,
            None,
        )
        .await
        .map_err(|e| StewardError::Store(format!("team_runner failed: {e}")))?;

        let now = now_ms();
        let artifact = EvolutionArtifact {
            id: crate::domain::new_id(),
            task_id: task.id.clone(),
            produced_by_role: task.dev_role.as_str().into(),
            artifact_type: match task.dev_role {
                DevRoleKind::Researcher => ArtifactType::ResearchReport,
                DevRoleKind::Designer => ArtifactType::DesignProposal,
                DevRoleKind::Developer => ArtifactType::ConfigChange,
                DevRoleKind::Tester => ArtifactType::TestReport,
                DevRoleKind::Verifier => ArtifactType::Verification,
            },
            content: serde_json::json!({
                "output": outcome.final_output,
                "converged": outcome.converged,
                "rounds": outcome.rounds,
            }),
            status: ArtifactStatus::PendingReview,
            diff_preview: None,
            rollback_plan: None,
            created_at: now,
        };
        let artifact_clone = artifact.clone();

        let task_id = task.id.clone();
        spawn_blocking(move || {
            let mut db = Db::open(&db_path)?;
            let conn = &mut db.0;
            let tx = conn.transaction()?;
            steward::insert_artifact(&tx, &artifact)?;
            steward::update_task_status(&tx, &task_id, StewardTaskStatus::Completed)?;
            tx.commit()?;
            Ok::<_, StewardError>(())
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))??;

        Ok(artifact_clone)
    }

    /// Dispatches all ready tasks in a cycle, looping until all complete or
    /// no more ready tasks (binding failure pauses without cancelling).
    pub async fn dispatch_cycle(
        db_path: Arc<str>,
        secrets: Arc<dyn SecretStore>,
        cycle_id: &str,
        bus: Option<EventBus>,
    ) -> Result<(), StewardError> {
        loop {
            let tasks = load_cycle_tasks(&db_path, cycle_id).await?;
            if tasks
                .iter()
                .all(|t| t.status == StewardTaskStatus::Completed)
            {
                return Ok(());
            }
            let graph = super::task::TaskDependencyGraph::new(tasks);
            let ready = graph.ready_tasks();
            if ready.is_empty() {
                return Ok(());
            }
            for task_id in ready {
                let task = graph.get(&task_id).cloned().ok_or(StewardError::NotFound {
                    entity: "evolution_task",
                    id: task_id.clone(),
                })?;
                match Self::run_task(db_path.clone(), secrets.clone(), task, bus.clone()).await {
                    Ok(_) => {}
                    Err(StewardError::DevTeamNotReady(msg)) => {
                        return Err(StewardError::DevTeamNotReady(msg));
                    }
                    Err(e) => {
                        let tid = task_id.clone();
                        let dp = db_path.clone();
                        spawn_blocking(move || {
                            let db = Db::open(&dp)?;
                            let conn = &db.0;
                            steward::update_task_status(conn, &tid, StewardTaskStatus::Failed)?;
                            Ok::<_, StewardError>(())
                        })
                        .await
                        .map_err(|e| StewardError::Store(format!("join error: {e}")))??;
                        return Err(e);
                    }
                }
            }
        }
    }
}

async fn ensure_temp_team(db_path: &Arc<str>, role_id: &str) -> Result<String, StewardError> {
    let role_id = role_id.to_string();
    let db_path = db_path.clone();
    spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        let conn = &db.0;
        let team_id = format!("steward_temp_{role_id}");
        if teams::get(conn, &team_id).is_ok() {
            return Ok(team_id);
        }
        let now = now_ms();
        let team = Team {
            id: team_id.clone(),
            name: format!("Steward temp team for {role_id}"),
            topology: TeamTopology::Pipeline,
            member_role_ids: vec![role_id],
            config: serde_json::json!({}),
            created_at: now,
            updated_at: now,
        };
        teams::insert(conn, &team)?;
        Ok(team_id)
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

async fn create_temp_session(db_path: &Arc<str>) -> Result<String, StewardError> {
    let db_path = db_path.clone();
    spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        let conn = &db.0;
        let now = now_ms();
        let session_id = crate::domain::new_id();
        let session = crate::domain::Session {
            id: session_id.clone(),
            title: "steward task dispatch".into(),
            created_at: now,
            updated_at: now,
            kind: crate::domain::ConversationKind::Background,
            team_id: None,
            task_id: None,
            schedule_id: None,
            goal: None,
            main_agent_id: None,
            route_mode: None,
            whiteboard_route_mode: None,
            deleted_at: None,
        };
        sessions::insert(conn, &session)?;
        Ok(session_id)
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

async fn load_cycle_tasks(
    db_path: &Arc<str>,
    cycle_id: &str,
) -> Result<Vec<EvolutionTask>, StewardError> {
    let cycle_id = cycle_id.to_string();
    let db_path = db_path.clone();
    spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        let conn = &db.0;
        steward::list_tasks_by_cycle(conn, &cycle_id).map_err(Into::into)
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[tokio::test]
    async fn ensure_dev_team_singleton_is_idempotent() {
        let dbp = db_path().await;
        let team1 = ensure_dev_team_singleton(dbp.clone()).await.unwrap();
        let team2 = ensure_dev_team_singleton(dbp.clone()).await.unwrap();
        assert_eq!(team1.id, team2.id);
        assert_eq!(team1.id, "steward_dev_team");

        let result = get_dev_team(dbp).await.unwrap();
        assert_eq!(result.bindings.len(), 5);
    }

    #[tokio::test]
    async fn set_dev_role_binding_replaces() {
        let dbp = db_path().await;
        ensure_dev_team_singleton(dbp.clone()).await.unwrap();

        let binding = DevRoleBinding {
            role_kind: DevRoleKind::Researcher,
            agent_kind: AgentRefKind::Role,
            agent_ref_id: "role_researcher_1".into(),
            updated_at: now_ms(),
        };
        set_dev_role_binding(dbp.clone(), DevRoleKind::Researcher, binding)
            .await
            .unwrap();

        let result = get_dev_team(dbp).await.unwrap();
        let researcher = result
            .bindings
            .iter()
            .find(|b| b.role_kind == DevRoleKind::Researcher)
            .unwrap();
        assert_eq!(researcher.agent_ref_id, "role_researcher_1");
    }

    #[tokio::test]
    async fn bindings_ready_checks_all_filled() {
        let empty = vec![DevRoleBinding {
            role_kind: DevRoleKind::Researcher,
            agent_kind: AgentRefKind::Role,
            agent_ref_id: String::new(),
            updated_at: 0,
        }];
        assert!(!bindings_ready(&empty));

        let filled = ALL_ROLE_KINDS.map(|kind| DevRoleBinding {
            role_kind: kind,
            agent_kind: AgentRefKind::Role,
            agent_ref_id: "r".into(),
            updated_at: 0,
        });
        assert!(bindings_ready(&filled));
    }

    #[tokio::test]
    async fn run_task_returns_not_ready_when_binding_empty() {
        let dbp = db_path().await;
        ensure_dev_team_singleton(dbp.clone()).await.unwrap();
        let task = EvolutionTask {
            id: crate::domain::new_id(),
            cycle_id: "c1".into(),
            phase: crate::domain::TaskPhase::Research,
            dev_role: DevRoleKind::Researcher,
            depends_on: vec![],
            status: StewardTaskStatus::Pending,
            acceptance_criteria: "test".into(),
            trigger_source: "test".into(),
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let err = DevTeamRunner::run_task(dbp, secrets, task, None)
            .await
            .unwrap_err();
        assert!(matches!(err, StewardError::DevTeamNotReady(_)));
    }

    #[tokio::test]
    async fn set_dev_role_binding_with_event_publishes() {
        let dbp = db_path().await;
        ensure_dev_team_singleton(dbp.clone()).await.unwrap();

        let binding = DevRoleBinding {
            role_kind: DevRoleKind::Developer,
            agent_kind: AgentRefKind::Role,
            agent_ref_id: "role_dev_1".into(),
            updated_at: now_ms(),
        };
        set_dev_role_binding_with_event(dbp.clone(), DevRoleKind::Developer, binding)
            .await
            .unwrap();

        let team = get_dev_team(dbp).await.unwrap();
        let dev = team
            .bindings
            .iter()
            .find(|b| b.role_kind == DevRoleKind::Developer)
            .unwrap();
        assert_eq!(dev.agent_ref_id, "role_dev_1");
    }

    #[tokio::test]
    async fn get_dev_team_returns_error_when_missing() {
        let dbp = db_path().await;
        let err = get_dev_team(dbp).await.unwrap_err();
        assert!(matches!(err, StewardError::NotFound { .. }));
    }

    #[tokio::test]
    async fn run_task_returns_not_ready_when_no_binding_for_role() {
        let dbp = db_path().await;
        ensure_dev_team_singleton(dbp.clone()).await.unwrap();

        let binding = DevRoleBinding {
            role_kind: DevRoleKind::Researcher,
            agent_kind: AgentRefKind::Role,
            agent_ref_id: "role_r1".into(),
            updated_at: now_ms(),
        };
        set_dev_role_binding(dbp.clone(), DevRoleKind::Researcher, binding)
            .await
            .unwrap();

        let task = EvolutionTask {
            id: crate::domain::new_id(),
            cycle_id: "c1".into(),
            phase: crate::domain::TaskPhase::Design,
            dev_role: DevRoleKind::Designer,
            depends_on: vec![],
            status: StewardTaskStatus::Pending,
            acceptance_criteria: "design".into(),
            trigger_source: "test".into(),
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let err = DevTeamRunner::run_task(dbp, secrets, task, None)
            .await
            .unwrap_err();
        assert!(matches!(err, StewardError::DevTeamNotReady(_)));
    }

    #[tokio::test]
    async fn ensure_dev_team_singleton_creates_all_5_bindings() {
        let dbp = db_path().await;
        let team = ensure_dev_team_singleton(dbp.clone()).await.unwrap();
        assert_eq!(team.id, "steward_dev_team");

        let result = get_dev_team(dbp).await.unwrap();
        assert_eq!(result.bindings.len(), 5);
        for kind in ALL_ROLE_KINDS {
            assert!(
                result.bindings.iter().any(|b| b.role_kind == kind),
                "missing binding for {kind:?}"
            );
        }
    }
}
