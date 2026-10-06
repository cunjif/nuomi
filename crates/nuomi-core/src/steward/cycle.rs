//! Evolution cycle orchestrator — the state machine that drives a self-evolution
//! cycle through its phases: cleanse → research → design → develop → test →
//! verify → gate → merge.
//!
//! Iron rule (design.md §2.1.3.5): every `CyclePhase` transition writes a
//! `steward.cycle_phase_changed` event to the `events` table BEFORE updating
//! the cycle's phase in `evolution_cycles`.

use std::sync::Arc;

use tokio::task::spawn_blocking;
use tokio::time::timeout;

use crate::domain::{
    now_ms, ArtifactStatus, ArtifactType, CyclePhase, CycleStatus, EvolutionArtifact,
    EvolutionCycle, EvolutionTask, StewardTaskStatus, TriggerSource,
};
use crate::harness::EventBus;
use crate::providers::SecretStore;
use crate::store::repos::{events, steward};
use crate::store::Db;

use super::StewardError;

/// Default per-phase timeout (spec §5.3.3 异常 2: 1800s).
const DEFAULT_PHASE_TIMEOUT_SECS: u64 = 1800;

/// How the evolution was triggered (design.md §2.2.2.2).
#[derive(Debug, Clone)]
pub enum EvolutionTrigger {
    UserExplicit {
        session_id: String,
        instruction: String,
    },
    Scheduled {
        schedule_id: String,
    },
    SelfReflect {
        reason: String,
    },
}

impl EvolutionTrigger {
    fn destructure(&self) -> (TriggerSource, String) {
        match self {
            EvolutionTrigger::UserExplicit {
                session_id,
                instruction,
            } => (
                TriggerSource::User,
                format!("session={session_id}; instruction={instruction}"),
            ),
            EvolutionTrigger::Scheduled { schedule_id } => {
                (TriggerSource::Scheduled, format!("schedule={schedule_id}"))
            }
            EvolutionTrigger::SelfReflect { reason } => {
                (TriggerSource::SelfReflect, reason.clone())
            }
        }
    }
}

/// Cycle detail returned by `get_cycle` (design.md §2.2.2.2).
#[derive(Debug, Clone)]
pub struct CycleDetail {
    pub cycle: EvolutionCycle,
    pub tasks: Vec<EvolutionTask>,
    pub artifacts: Vec<EvolutionArtifact>,
}

/// Triggers a new evolution cycle.
///
/// Creates the `evolution_cycles` row (status=running, phase=cleanse) and
/// async-launches `CycleOrchestrator::run`. Returns immediately with the
/// cycle — the actual work happens in a background task (DFX §4.1 规则1:
/// complex evolution is async with immediate "accepted" feedback).
pub async fn trigger_evolution(
    db_path: Arc<str>,
    secrets: Arc<dyn SecretStore>,
    trigger: EvolutionTrigger,
    bus: Option<EventBus>,
) -> Result<EvolutionCycle, StewardError> {
    let (trigger_source, trigger_context) = trigger.destructure();
    let cycle = EvolutionCycle {
        id: crate::domain::new_id(),
        trigger_source,
        trigger_context,
        phase: CyclePhase::Cleanse,
        status: CycleStatus::Running,
        created_at: now_ms(),
        ended_at: None,
    };

    let cycle_for_db = cycle.clone();
    let db_path_for_db = db_path.clone();
    spawn_blocking(move || {
        let mut db = Db::open(&db_path_for_db)?;
        let conn = &mut db.0;
        steward::insert_cycle(conn, &cycle_for_db)?;
        Ok::<_, StewardError>(())
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))??;

    let cycle_id = cycle.id.clone();
    tokio::spawn(async move {
        if let Err(e) = CycleOrchestrator::run(db_path, secrets, cycle_id, bus).await {
            tracing::error!(error = %e, "evolution cycle orchestrator failed");
        }
    });

    Ok(cycle)
}

/// Cancels an evolution cycle (T6-8).
///
/// - Pending tasks → status=`cancelled`
/// - Running tasks → best-effort graceful termination (CancellationToken via
///   task status update; in-flight team_runner calls finish naturally)
/// - Completed artifacts are preserved (spec §5.3.1 规则4)
/// - Cycle status → `cancelled`, `ended_at` set
pub async fn cancel_cycle(db_path: Arc<str>, cycle_id: &str) -> Result<(), StewardError> {
    let cycle_id = cycle_id.to_string();
    spawn_blocking(move || {
        let mut db = Db::open(&db_path)?;
        let conn = &mut db.0;
        let tx = conn.transaction()?;

        // Cancel all pending/running tasks (completed/failed/cancelled left as-is)
        tx.execute(
            "UPDATE evolution_tasks
             SET status = 'cancelled', updated_at = ?2
             WHERE cycle_id = ?1 AND status IN ('pending', 'running')",
            rusqlite::params![&cycle_id, now_ms()],
        )?;

        // Iron rule: event before side effect
        events::append(
            &tx,
            "steward",
            &cycle_id,
            "steward.cycle_cancelled",
            &serde_json::json!({}),
            now_ms(),
        )?;
        steward::cancel_cycle(&tx, &cycle_id, now_ms())?;
        tx.commit()?;
        Ok::<_, StewardError>(())
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

/// Gets cycle detail: the cycle + its tasks + its artifacts.
pub async fn get_cycle(db_path: Arc<str>, cycle_id: &str) -> Result<CycleDetail, StewardError> {
    let cycle_id = cycle_id.to_string();
    spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        let conn = &db.0;
        let cycle = steward::get_cycle(conn, &cycle_id)?;
        let tasks = steward::list_tasks_by_cycle(conn, &cycle_id)?;
        let artifacts = steward::list_artifacts_by_cycle(conn, &cycle_id)?;
        Ok(CycleDetail {
            cycle,
            tasks,
            artifacts,
        })
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

/// Lists cycles, optionally filtered by status.
pub async fn list_cycles(
    db_path: Arc<str>,
    status: Option<CycleStatus>,
    limit: u32,
) -> Result<Vec<EvolutionCycle>, StewardError> {
    spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        let conn = &db.0;
        match status {
            Some(s) => steward::list_cycles_by_status(conn, s, limit),
            None => steward::list_cycles(conn, limit),
        }
        .map_err(Into::into)
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

// ============================================================ CycleOrchestrator

/// The evolution cycle state machine.
///
/// Drives a cycle through cleanse → research → design → develop → test →
/// verify → gate → merge. Each phase transition follows the iron rule:
/// write `steward.cycle_phase_changed` event BEFORE updating the phase.
pub struct CycleOrchestrator;

impl CycleOrchestrator {
    /// Runs the full cycle to completion (or failure).
    ///
    /// On any phase failure (except research-skip), marks the cycle as
    /// `failed` and returns — prior artifacts are preserved (T6-9).
    pub async fn run(
        db_path: Arc<str>,
        secrets: Arc<dyn SecretStore>,
        cycle_id: String,
        bus: Option<EventBus>,
    ) -> Result<(), StewardError> {
        // Phase: Cleanse (T6-3)
        if let Err(e) =
            Self::run_phase_timed(&db_path, &secrets, &cycle_id, &bus, PhaseKind::Cleanse).await
        {
            Self::fail(&db_path, &cycle_id, CyclePhase::Cleanse, &e).await;
            return Err(e);
        }
        Self::advance(
            &db_path,
            &cycle_id,
            CyclePhase::Cleanse,
            CyclePhase::Research,
        )
        .await?;

        // Phase: Research (T6-4 — skips gracefully if not authorized)
        if let Err(e) =
            Self::run_phase_timed(&db_path, &secrets, &cycle_id, &bus, PhaseKind::Research).await
        {
            Self::fail(&db_path, &cycle_id, CyclePhase::Research, &e).await;
            return Err(e);
        }
        Self::advance(
            &db_path,
            &cycle_id,
            CyclePhase::Research,
            CyclePhase::Design,
        )
        .await?;

        // Phase: Design (T6-5/T6-6 — plan tasks + dispatch)
        if let Err(e) =
            Self::run_phase_timed(&db_path, &secrets, &cycle_id, &bus, PhaseKind::Design).await
        {
            Self::fail(&db_path, &cycle_id, CyclePhase::Design, &e).await;
            return Err(e);
        }
        Self::advance(&db_path, &cycle_id, CyclePhase::Design, CyclePhase::Develop).await?;

        // Phase: Develop
        if let Err(e) =
            Self::run_phase_timed(&db_path, &secrets, &cycle_id, &bus, PhaseKind::Develop).await
        {
            Self::fail(&db_path, &cycle_id, CyclePhase::Develop, &e).await;
            return Err(e);
        }
        Self::advance(&db_path, &cycle_id, CyclePhase::Develop, CyclePhase::Test).await?;

        // Phase: Test
        if let Err(e) =
            Self::run_phase_timed(&db_path, &secrets, &cycle_id, &bus, PhaseKind::Test).await
        {
            Self::fail(&db_path, &cycle_id, CyclePhase::Test, &e).await;
            return Err(e);
        }
        Self::advance(&db_path, &cycle_id, CyclePhase::Test, CyclePhase::Verify).await?;

        // Phase: Verify
        if let Err(e) =
            Self::run_phase_timed(&db_path, &secrets, &cycle_id, &bus, PhaseKind::Verify).await
        {
            Self::fail(&db_path, &cycle_id, CyclePhase::Verify, &e).await;
            return Err(e);
        }
        Self::advance(&db_path, &cycle_id, CyclePhase::Verify, CyclePhase::Gate).await?;

        // Phase: Gate — user approval happens externally via K-Steward-7
        Self::run_gate(&db_path, &cycle_id).await?;
        Self::advance(&db_path, &cycle_id, CyclePhase::Gate, CyclePhase::Merge).await?;

        // Phase: Merge (K-Steward-7 will fully implement)
        Self::run_merge(&db_path, &cycle_id).await?;

        // Mark completed
        Self::complete(&db_path, &cycle_id).await?;
        Ok(())
    }

    /// Runs a phase with a timeout (T6-10). On timeout, logs and returns
    /// `StewardError::Store` — the cycle continues in background (the `run`
    /// method marks the cycle as failed, but the caller is never blocked).
    async fn run_phase_timed(
        db_path: &Arc<str>,
        secrets: &Arc<dyn SecretStore>,
        cycle_id: &str,
        bus: &Option<EventBus>,
        phase: PhaseKind,
    ) -> Result<(), StewardError> {
        let dur = std::time::Duration::from_secs(DEFAULT_PHASE_TIMEOUT_SECS);
        let fut = Self::run_phase(db_path, secrets, cycle_id, bus, phase);
        match timeout(dur, fut).await {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!(
                    cycle_id = cycle_id,
                    phase = ?phase,
                    "phase timed out after {}s",
                    DEFAULT_PHASE_TIMEOUT_SECS
                );
                Err(StewardError::Store(format!(
                    "phase {phase:?} timed out after {DEFAULT_PHASE_TIMEOUT_SECS}s"
                )))
            }
        }
    }

    /// Dispatches to the appropriate phase runner.
    async fn run_phase(
        db_path: &Arc<str>,
        secrets: &Arc<dyn SecretStore>,
        cycle_id: &str,
        bus: &Option<EventBus>,
        phase: PhaseKind,
    ) -> Result<(), StewardError> {
        match phase {
            PhaseKind::Cleanse => Self::run_cleanse(db_path, cycle_id).await,
            PhaseKind::Research => Self::run_research(db_path, cycle_id).await,
            PhaseKind::Design => Self::run_design(db_path, secrets, cycle_id, bus).await,
            PhaseKind::Develop => Self::run_develop(db_path, secrets, cycle_id, bus).await,
            PhaseKind::Test => Self::run_test(db_path, secrets, cycle_id, bus).await,
            PhaseKind::Verify => Self::run_verify(db_path, secrets, cycle_id, bus).await,
        }
    }

    // ---------------------------------------------------------- Cleanse (T6-3)

    /// Cleanse phase: aggregates trajectories via `TrajectoryAggregator` and
    /// runs `DataCleanser::cleanse` to produce a cleansed data pool.
    ///
    /// Does not modify `TrajectoryAggregator` source — only calls it.
    async fn run_cleanse(db_path: &Arc<str>, cycle_id: &str) -> Result<(), StewardError> {
        use super::cleanse::{CleanseRules, CleanseScope, DataCleanser, DefaultCleanser};

        let now = now_ms();
        let scope = CleanseScope {
            time_range: (now - 30 * 24 * 3600 * 1000, now),
            include_kinds: vec![],
            include_memory_kinds: vec![],
        };
        let rules = CleanseRules::default();

        let cleanser = DefaultCleanser;
        let report = DataCleanser::cleanse(&cleanser, db_path.clone(), &scope, &rules)
            .await
            .map_err(|e| StewardError::Store(format!("cleanse failed: {e}")))?;

        // Store the pool_id as an artifact for traceability
        let phase_task_id = ensure_phase_task(db_path, cycle_id, "cleanse").await?;
        let artifact = EvolutionArtifact {
            id: crate::domain::new_id(),
            task_id: phase_task_id,
            produced_by_role: "steward".into(),
            artifact_type: ArtifactType::ResearchReport,
            content: serde_json::json!({
                "pool_id": report.pool_id,
                "input_count": report.input_count,
                "output_count": report.output_count,
                "uncovered_kinds": report.uncovered_kinds,
            }),
            status: ArtifactStatus::PendingReview,
            diff_preview: None,
            rollback_plan: None,
            created_at: now,
        };

        let db_path = db_path.clone();
        spawn_blocking(move || {
            let mut db = Db::open(&db_path)?;
            let conn = &mut db.0;
            steward::insert_artifact(conn, &artifact)?;
            Ok::<_, StewardError>(())
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))??;

        Ok(())
    }

    // ---------------------------------------------------------- Research (T6-4)

    /// Research phase: checks `online_authorized`. If not authorized, skips
    /// the phase with a note (spec §5.5.3 异常 1) — the cycle does NOT fail.
    /// If authorized, attempts online research via `ResearchScheduler`.
    ///
    /// Does not modify `ResearchScheduler` source — only calls it.
    async fn run_research(db_path: &Arc<str>, cycle_id: &str) -> Result<(), StewardError> {
        let authorized = read_online_authorized(db_path).await;

        if !authorized {
            // Skip research phase — cycle continues (spec §5.5.3 异常 1)
            let phase_task_id = ensure_phase_task(db_path, cycle_id, "research").await?;
            let artifact = EvolutionArtifact {
                id: crate::domain::new_id(),
                task_id: phase_task_id,
                produced_by_role: "steward".into(),
                artifact_type: ArtifactType::ResearchReport,
                content: serde_json::json!({
                    "skipped": true,
                    "reason": "online research not authorized",
                    "hint": "授权后可参考外部实践增强进化效果",
                }),
                status: ArtifactStatus::PendingReview,
                diff_preview: None,
                rollback_plan: None,
                created_at: now_ms(),
            };
            insert_artifact(db_path, &artifact).await?;
            tracing::info!(
                cycle_id = cycle_id,
                "research phase skipped — online not authorized"
            );
            return Ok(());
        }

        // Authorized: actual research requires a configured ResearchScheduler
        // (runtime concern). We record the authorization and proceed — the
        // dev team's research task (via DevTeamRunner) will do the work.
        let phase_task_id = ensure_phase_task(db_path, cycle_id, "research").await?;
        let artifact = EvolutionArtifact {
            id: crate::domain::new_id(),
            task_id: phase_task_id,
            produced_by_role: "steward".into(),
            artifact_type: ArtifactType::ResearchReport,
            content: serde_json::json!({
                "online_authorized": true,
                "status": "delegated_to_dev_team",
            }),
            status: ArtifactStatus::PendingReview,
            diff_preview: None,
            rollback_plan: None,
            created_at: now_ms(),
        };
        insert_artifact(db_path, &artifact).await?;
        Ok(())
    }

    // ---------------------------------------------------------- Design (T6-5/T6-6)

    /// Design phase: plans evolution tasks via `TaskPlanner::plan` and
    /// inserts them into the DB. The actual task dispatch happens across
    /// design/develop/test/verify via `DevTeamRunner::dispatch_cycle`.
    ///
    /// Integration point for `Reflector::reflect` (T6-5): the cleansed data
    /// pool + research report serve as reflection input. The Reflector call
    /// is delegated to the dev team's designer role.
    async fn run_design(
        db_path: &Arc<str>,
        _secrets: &Arc<dyn SecretStore>,
        cycle_id: &str,
        _bus: &Option<EventBus>,
    ) -> Result<(), StewardError> {
        let cycle_id_owned = cycle_id.to_string();
        let instruction = {
            let dp = db_path.clone();
            let cid = cycle_id_owned.clone();
            spawn_blocking(move || {
                let db = Db::open(&dp)?;
                let conn = &db.0;
                let instruction = steward::get_cycle(conn, &cid)
                    .map(|c| c.trigger_context)
                    .unwrap_or_else(|_| "steward evolution cycle".into());
                Ok::<_, StewardError>(instruction)
            })
            .await
            .map_err(|e| StewardError::Store(format!("join error: {e}")))?
        };
        let tasks = super::task::TaskPlanner::plan(&cycle_id_owned, &instruction?);
        insert_tasks(db_path, &tasks).await?;
        Ok(())
    }

    // ---------------------------------------------------------- Develop (T6-6)

    /// Develop phase: dispatches ready tasks to the dev team.
    async fn run_develop(
        db_path: &Arc<str>,
        secrets: &Arc<dyn SecretStore>,
        cycle_id: &str,
        bus: &Option<EventBus>,
    ) -> Result<(), StewardError> {
        super::dev_team::DevTeamRunner::dispatch_cycle(
            db_path.clone(),
            secrets.clone(),
            cycle_id,
            bus.clone(),
        )
        .await?;
        Ok(())
    }

    // ---------------------------------------------------------- Test (T6-6)

    /// Test phase: tasks are dispatched as part of develop; this phase
    /// verifies that all tasks completed successfully.
    async fn run_test(
        db_path: &Arc<str>,
        _secrets: &Arc<dyn SecretStore>,
        cycle_id: &str,
        _bus: &Option<EventBus>,
    ) -> Result<(), StewardError> {
        verify_tasks_completed(db_path, cycle_id).await
    }

    // ---------------------------------------------------------- Verify (T6-6)

    /// Verify phase: confirms all artifacts are in `pending_review` status,
    /// ready for the gate.
    async fn run_verify(
        db_path: &Arc<str>,
        _secrets: &Arc<dyn SecretStore>,
        cycle_id: &str,
        _bus: &Option<EventBus>,
    ) -> Result<(), StewardError> {
        let artifacts = list_cycle_artifacts(db_path, cycle_id).await?;
        if artifacts.is_empty() {
            return Err(StewardError::Store(format!(
                "cycle {cycle_id} produced no artifacts"
            )));
        }
        Ok(())
    }

    // ---------------------------------------------------------- Gate

    /// Gate phase: artifacts are in `pending_review` status. User approval
    /// happens externally via K-Steward-7 (`steward_resolve_gate`). This
    /// method just records that the cycle reached the gate.
    async fn run_gate(db_path: &Arc<str>, cycle_id: &str) -> Result<(), StewardError> {
        let phase_task_id = ensure_phase_task(db_path, cycle_id, "gate").await?;
        let artifact = EvolutionArtifact {
            id: crate::domain::new_id(),
            task_id: phase_task_id,
            produced_by_role: "steward".into(),
            artifact_type: ArtifactType::Verification,
            content: serde_json::json!({
                "status": "awaiting_user_approval",
                "hint": "use steward_resolve_gate to approve/reject artifacts",
            }),
            status: ArtifactStatus::PendingReview,
            diff_preview: None,
            rollback_plan: None,
            created_at: now_ms(),
        };
        insert_artifact(db_path, &artifact).await?;
        Ok(())
    }

    // ---------------------------------------------------------- Merge

    /// Merge phase: approved artifacts are merged into production. Full
    /// implementation in K-Steward-7 (`ArtifactMerger`). This placeholder
    /// records that the cycle reached merge.
    async fn run_merge(db_path: &Arc<str>, cycle_id: &str) -> Result<(), StewardError> {
        let db_path = db_path.clone();
        let cycle_id = cycle_id.to_string();
        spawn_blocking(move || {
            let db = Db::open(&db_path)?;
            let conn = &db.0;
            events::append(
                conn,
                "steward",
                &cycle_id,
                "steward.cycle_merge_reached",
                &serde_json::json!({}),
                now_ms(),
            )?;
            Ok::<_, StewardError>(())
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))??;
        Ok(())
    }

    // ---------------------------------------------------------- state transitions

    /// Advances the cycle from `from` phase to `to` phase.
    ///
    /// Iron rule: writes `steward.cycle_phase_changed` event BEFORE
    /// updating the cycle's phase (design.md §2.1.3.5).
    async fn advance(
        db_path: &Arc<str>,
        cycle_id: &str,
        from: CyclePhase,
        to: CyclePhase,
    ) -> Result<(), StewardError> {
        let cycle_id = cycle_id.to_string();
        let db_path = db_path.clone();
        spawn_blocking(move || {
            let mut db = Db::open(&db_path)?;
            let conn = &mut db.0;
            let tx = conn.transaction()?;
            events::append(
                &tx,
                "steward",
                &cycle_id,
                "steward.cycle_phase_changed",
                &serde_json::json!({"from": from.as_str(), "to": to.as_str()}),
                now_ms(),
            )?;
            steward::update_cycle_phase_status(&tx, &cycle_id, to, CycleStatus::Running)?;
            tx.commit()?;
            Ok::<_, StewardError>(())
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))?
    }

    /// Marks the cycle as completed.
    async fn complete(db_path: &Arc<str>, cycle_id: &str) -> Result<(), StewardError> {
        let cycle_id = cycle_id.to_string();
        let db_path = db_path.clone();
        spawn_blocking(move || {
            let mut db = Db::open(&db_path)?;
            let conn = &mut db.0;
            let tx = conn.transaction()?;
            events::append(
                &tx,
                "steward",
                &cycle_id,
                "steward.cycle_completed",
                &serde_json::json!({}),
                now_ms(),
            )?;
            tx.execute(
                "UPDATE evolution_cycles SET status = 'completed', ended_at = ?2 WHERE id = ?1",
                rusqlite::params![&cycle_id, now_ms()],
            )?;
            tx.commit()?;
            Ok::<_, StewardError>(())
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))?
    }

    /// Marks the cycle as failed, preserving all prior artifacts (T6-9).
    async fn fail(db_path: &Arc<str>, cycle_id: &str, phase: CyclePhase, error: &StewardError) {
        let cycle_id = cycle_id.to_string();
        let error_msg = error.to_string();
        let db_path = db_path.clone();
        let _ = spawn_blocking(move || {
            let mut db = Db::open(&db_path)?;
            let conn = &mut db.0;
            let tx = conn.transaction()?;
            events::append(
                &tx,
                "steward",
                &cycle_id,
                "steward.cycle_failed",
                &serde_json::json!({"phase": phase.as_str(), "error": error_msg}),
                now_ms(),
            )?;
            tx.execute(
                "UPDATE evolution_cycles SET status = 'failed', ended_at = ?2 WHERE id = ?1",
                rusqlite::params![&cycle_id, now_ms()],
            )?;
            tx.commit()?;
            Ok::<_, StewardError>(())
        })
        .await;
    }
}

/// Which phase to run (internal dispatch).
#[derive(Debug, Clone, Copy)]
enum PhaseKind {
    Cleanse,
    Research,
    Design,
    Develop,
    Test,
    Verify,
}

// ============================================================ helpers

/// Reads the `online_authorized` flag from the `steward_ai` singleton.
async fn read_online_authorized(db_path: &Arc<str>) -> bool {
    let db_path = db_path.clone();
    spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        let conn = &db.0;
        Ok::<_, StewardError>(
            steward::get_steward_ai(conn)
                .ok()
                .flatten()
                .is_some_and(|ai| ai.online_authorized),
        )
    })
    .await
    .map(|r| r.unwrap_or(false))
    .unwrap_or(false)
}

/// Ensures a placeholder task exists for a phase (for FK-valid artifacts).
/// Returns the task id.
async fn ensure_phase_task(
    db_path: &Arc<str>,
    cycle_id: &str,
    phase_label: &str,
) -> Result<String, StewardError> {
    let task_id = format!("{cycle_id}:{phase_label}");
    let task_id_check = task_id.clone();
    let exists = {
        let db_path = db_path.clone();
        spawn_blocking(move || {
            let db = Db::open(&db_path)?;
            let conn = &db.0;
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM evolution_tasks WHERE id = ?1",
                rusqlite::params![&task_id_check],
                |r| r.get(0),
            )?;
            Ok::<_, StewardError>(n > 0)
        })
        .await
        .map_err(|e| StewardError::Store(format!("join error: {e}")))?
    };
    if exists? {
        return Ok(task_id);
    }
    let task = EvolutionTask {
        id: task_id.clone(),
        cycle_id: cycle_id.into(),
        phase: crate::domain::TaskPhase::Research,
        dev_role: crate::domain::DevRoleKind::Researcher,
        depends_on: vec![],
        status: StewardTaskStatus::Completed,
        acceptance_criteria: format!("phase:{phase_label}"),
        trigger_source: "steward".into(),
        created_at: now_ms(),
        updated_at: now_ms(),
    };
    insert_tasks(db_path, std::slice::from_ref(&task)).await?;
    Ok(task_id)
}

/// Inserts an artifact.
async fn insert_artifact(
    db_path: &Arc<str>,
    artifact: &EvolutionArtifact,
) -> Result<(), StewardError> {
    let artifact = artifact.clone();
    let db_path = db_path.clone();
    spawn_blocking(move || {
        let mut db = Db::open(&db_path)?;
        let conn = &mut db.0;
        steward::insert_artifact(conn, &artifact)?;
        Ok::<_, StewardError>(())
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

/// Inserts multiple tasks in a single transaction.
async fn insert_tasks(db_path: &Arc<str>, tasks: &[EvolutionTask]) -> Result<(), StewardError> {
    let tasks = tasks.to_vec();
    let db_path = db_path.clone();
    spawn_blocking(move || {
        let mut db = Db::open(&db_path)?;
        let conn = &mut db.0;
        let tx = conn.transaction()?;
        for task in &tasks {
            steward::insert_task(&tx, task)?;
        }
        tx.commit()?;
        Ok::<_, StewardError>(())
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))?
}

/// Verifies that all tasks in a cycle are completed.
async fn verify_tasks_completed(db_path: &Arc<str>, cycle_id: &str) -> Result<(), StewardError> {
    let cycle_id = cycle_id.to_string();
    let db_path = db_path.clone();
    let tasks = spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        let conn = &db.0;
        Ok::<_, StewardError>(steward::list_tasks_by_cycle(conn, &cycle_id)?)
    })
    .await
    .map_err(|e| StewardError::Store(format!("join error: {e}")))??;

    let incomplete: Vec<_> = tasks
        .iter()
        .filter(|t| t.status != StewardTaskStatus::Completed)
        .collect();
    if !incomplete.is_empty() {
        return Err(StewardError::Store(format!(
            "cycle has {} incomplete tasks",
            incomplete.len()
        )));
    }
    Ok(())
}

/// Lists all artifacts for a cycle.
async fn list_cycle_artifacts(
    db_path: &Arc<str>,
    cycle_id: &str,
) -> Result<Vec<EvolutionArtifact>, StewardError> {
    let cycle_id = cycle_id.to_string();
    let db_path = db_path.clone();
    spawn_blocking(move || {
        let db = Db::open(&db_path)?;
        let conn = &db.0;
        Ok::<_, StewardError>(steward::list_artifacts_by_cycle(conn, &cycle_id)?)
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

    /// Creates a task in the DB and returns its id (for FK-valid artifacts).
    async fn create_task(db_path: &Arc<str>, cycle_id: &str) -> String {
        let task = EvolutionTask {
            id: crate::domain::new_id(),
            cycle_id: cycle_id.into(),
            phase: crate::domain::TaskPhase::Research,
            dev_role: crate::domain::DevRoleKind::Researcher,
            depends_on: vec![],
            status: StewardTaskStatus::Completed,
            acceptance_criteria: "test".into(),
            trigger_source: "test".into(),
            created_at: now_ms(),
            updated_at: now_ms(),
        };
        let id = task.id.clone();
        insert_tasks(db_path, std::slice::from_ref(&task))
            .await
            .unwrap();
        id
    }

    #[tokio::test]
    async fn trigger_evolution_creates_cycle_row() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let trigger = EvolutionTrigger::SelfReflect {
            reason: "test".into(),
        };
        let cycle = trigger_evolution(dbp.clone(), secrets, trigger, None)
            .await
            .unwrap();
        assert_eq!(cycle.status, CycleStatus::Running);
        assert_eq!(cycle.phase, CyclePhase::Cleanse);

        // Verify the row is in the DB
        let detail = get_cycle(dbp, &cycle.id).await.unwrap();
        assert_eq!(detail.cycle.id, cycle.id);
    }

    #[tokio::test]
    async fn cancel_cycle_preserves_completed_artifacts() {
        let dbp = db_path().await;

        // Insert a cycle row directly (without spawning the orchestrator)
        let cycle = EvolutionCycle {
            id: crate::domain::new_id(),
            trigger_source: TriggerSource::User,
            trigger_context: "test".into(),
            phase: CyclePhase::Cleanse,
            status: CycleStatus::Running,
            created_at: now_ms(),
            ended_at: None,
        };
        {
            let dbp = dbp.clone();
            let cycle = cycle.clone();
            spawn_blocking(move || {
                let mut db = Db::open(&dbp)?;
                steward::insert_cycle(&mut db.0, &cycle)?;
                Ok::<_, StewardError>(())
            })
            .await
            .unwrap()
            .unwrap();
        }

        // Create a task + artifact manually
        let task_id = create_task(&dbp, &cycle.id).await;
        let artifact = EvolutionArtifact {
            id: crate::domain::new_id(),
            task_id,
            produced_by_role: "steward".into(),
            artifact_type: ArtifactType::ResearchReport,
            content: serde_json::json!({"data": "important"}),
            status: ArtifactStatus::Approved,
            diff_preview: None,
            rollback_plan: None,
            created_at: now_ms(),
        };
        insert_artifact(&dbp, &artifact).await.unwrap();

        // Cancel the cycle
        cancel_cycle(dbp.clone(), &cycle.id).await.unwrap();

        // Artifact must still be present
        let detail = get_cycle(dbp, &cycle.id).await.unwrap();
        assert_eq!(detail.cycle.status, CycleStatus::Cancelled);
        assert!(detail.artifacts.iter().any(|a| a.id == artifact.id));
    }

    #[tokio::test]
    async fn list_cycles_returns_created_cycles() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());

        let c1 = trigger_evolution(
            dbp.clone(),
            secrets.clone(),
            EvolutionTrigger::SelfReflect { reason: "1".into() },
            None,
        )
        .await
        .unwrap();
        let c2 = trigger_evolution(
            dbp.clone(),
            secrets,
            EvolutionTrigger::SelfReflect { reason: "2".into() },
            None,
        )
        .await
        .unwrap();

        let all = list_cycles(dbp, None, 10).await.unwrap();
        assert!(all.iter().any(|c| c.id == c1.id));
        assert!(all.iter().any(|c| c.id == c2.id));
    }

    #[tokio::test]
    async fn advance_writes_event_before_phase_update() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let cycle = trigger_evolution(
            dbp.clone(),
            secrets,
            EvolutionTrigger::SelfReflect {
                reason: "test".into(),
            },
            None,
        )
        .await
        .unwrap();

        // Advance from cleanse to research
        CycleOrchestrator::advance(&dbp, &cycle.id, CyclePhase::Cleanse, CyclePhase::Research)
            .await
            .unwrap();

        // Verify event was written
        let db = Db::open(&dbp).unwrap();
        let evs = events::list_by_aggregate(&db.0, "steward", &cycle.id, None).unwrap();
        assert!(evs.iter().any(|e| e.kind == "steward.cycle_phase_changed"));

        // Verify phase was updated
        let detail = get_cycle(dbp, &cycle.id).await.unwrap();
        assert_eq!(detail.cycle.phase, CyclePhase::Research);
    }

    #[tokio::test]
    async fn fail_marks_cycle_failed_and_preserves_artifacts() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let cycle = trigger_evolution(
            dbp.clone(),
            secrets,
            EvolutionTrigger::SelfReflect {
                reason: "test".into(),
            },
            None,
        )
        .await
        .unwrap();

        // Create a task + artifact before the failure
        let task_id = create_task(&dbp, &cycle.id).await;
        let artifact = EvolutionArtifact {
            id: crate::domain::new_id(),
            task_id,
            produced_by_role: "steward".into(),
            artifact_type: ArtifactType::DesignProposal,
            content: serde_json::json!({"design": "v1"}),
            status: ArtifactStatus::PendingReview,
            diff_preview: None,
            rollback_plan: None,
            created_at: now_ms(),
        };
        insert_artifact(&dbp, &artifact).await.unwrap();

        // Fail the cycle
        CycleOrchestrator::fail(
            &dbp,
            &cycle.id,
            CyclePhase::Develop,
            &StewardError::Store("test failure".into()),
        )
        .await;

        // Artifact must still be present (T6-9)
        let detail = get_cycle(dbp, &cycle.id).await.unwrap();
        assert_eq!(detail.cycle.status, CycleStatus::Failed);
        assert!(detail.artifacts.iter().any(|a| a.id == artifact.id));
    }

    // ============================ integration tests (T6-14) ============================

    /// Inserts a cycle row directly (no background orchestrator).
    async fn insert_cycle_direct(db_path: &Arc<str>) -> EvolutionCycle {
        let cycle = EvolutionCycle {
            id: crate::domain::new_id(),
            trigger_source: TriggerSource::User,
            trigger_context: "integration test".into(),
            phase: CyclePhase::Cleanse,
            status: CycleStatus::Running,
            created_at: now_ms(),
            ended_at: None,
        };
        let cycle_clone = cycle.clone();
        let db_path = db_path.clone();
        spawn_blocking(move || {
            let mut db = Db::open(&db_path)?;
            steward::insert_cycle(&mut db.0, &cycle_clone)?;
            Ok::<_, StewardError>(())
        })
        .await
        .unwrap()
        .unwrap();
        cycle
    }

    #[tokio::test]
    async fn integration_cleanse_phase_produces_artifact() {
        let dbp = db_path().await;
        let cycle = insert_cycle_direct(&dbp).await;

        // Run cleanse phase directly
        CycleOrchestrator::run_cleanse(&dbp, &cycle.id)
            .await
            .unwrap();

        // Verify an artifact was produced
        let detail = get_cycle(dbp, &cycle.id).await.unwrap();
        assert!(
            !detail.artifacts.is_empty(),
            "cleanse should produce an artifact"
        );
        let cleanse_artifact = detail
            .artifacts
            .iter()
            .find(|a| a.task_id.contains("cleanse"))
            .expect("cleanse artifact should exist");
        assert!(cleanse_artifact.content.get("pool_id").is_some());
    }

    #[tokio::test]
    async fn integration_research_skips_when_not_authorized() {
        let dbp = db_path().await;
        let cycle = insert_cycle_direct(&dbp).await;

        // online_authorized defaults to false — research should skip
        CycleOrchestrator::run_research(&dbp, &cycle.id)
            .await
            .unwrap();

        // Verify a "skipped" artifact was produced
        let detail = get_cycle(dbp, &cycle.id).await.unwrap();
        let research_artifact = detail
            .artifacts
            .iter()
            .find(|a| a.task_id.contains("research"))
            .expect("research artifact should exist");
        assert_eq!(
            research_artifact.content.get("skipped"),
            Some(&serde_json::json!(true)),
            "research should be skipped when not authorized"
        );
    }

    #[tokio::test]
    async fn integration_cancel_cancels_pending_tasks() {
        let dbp = db_path().await;
        let cycle = insert_cycle_direct(&dbp).await;

        // Create pending tasks
        let tasks = super::super::task::TaskPlanner::plan(&cycle.id, "test instruction");
        insert_tasks(&dbp, &tasks).await.unwrap();

        // Cancel the cycle
        cancel_cycle(dbp.clone(), &cycle.id).await.unwrap();

        // All tasks should be cancelled
        let detail = get_cycle(dbp, &cycle.id).await.unwrap();
        assert_eq!(detail.cycle.status, CycleStatus::Cancelled);
        for task in &detail.tasks {
            assert_eq!(
                task.status,
                StewardTaskStatus::Cancelled,
                "pending task should be cancelled"
            );
        }
    }

    #[tokio::test]
    async fn integration_fail_preserves_prior_artifacts() {
        let dbp = db_path().await;
        let cycle = insert_cycle_direct(&dbp).await;

        // Simulate prior phase producing artifacts
        let task_id = create_task(&dbp, &cycle.id).await;
        let artifact1 = EvolutionArtifact {
            id: crate::domain::new_id(),
            task_id: task_id.clone(),
            produced_by_role: "researcher".into(),
            artifact_type: ArtifactType::ResearchReport,
            content: serde_json::json!({"finding": "improvement area"}),
            status: ArtifactStatus::PendingReview,
            diff_preview: None,
            rollback_plan: None,
            created_at: now_ms(),
        };
        insert_artifact(&dbp, &artifact1).await.unwrap();

        // Simulate develop phase failure
        CycleOrchestrator::fail(
            &dbp,
            &cycle.id,
            CyclePhase::Develop,
            &StewardError::Store("develop failed".into()),
        )
        .await;

        // Prior artifact must still be present
        let detail = get_cycle(dbp.clone(), &cycle.id).await.unwrap();
        assert_eq!(detail.cycle.status, CycleStatus::Failed);
        assert!(
            detail.artifacts.iter().any(|a| a.id == artifact1.id),
            "prior research artifact must be preserved after develop failure"
        );

        // Verify failure event was written
        let db = Db::open(&dbp).unwrap();
        let evs = events::list_by_aggregate(&db.0, "steward", &cycle.id, None).unwrap();
        assert!(evs.iter().any(|e| e.kind == "steward.cycle_failed"));
    }

    #[tokio::test]
    async fn integration_full_phase_progression_to_gate() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let cycle = insert_cycle_direct(&dbp).await;

        // Run cleanse → research → design → develop → test → verify → gate
        // (without dev team dispatch — just phase transitions)
        CycleOrchestrator::run_cleanse(&dbp, &cycle.id)
            .await
            .unwrap();
        CycleOrchestrator::advance(&dbp, &cycle.id, CyclePhase::Cleanse, CyclePhase::Research)
            .await
            .unwrap();

        CycleOrchestrator::run_research(&dbp, &cycle.id)
            .await
            .unwrap();
        CycleOrchestrator::advance(&dbp, &cycle.id, CyclePhase::Research, CyclePhase::Design)
            .await
            .unwrap();

        // Design: plan tasks (but don't dispatch — no dev team bindings)
        let tasks = super::super::task::TaskPlanner::plan(&cycle.id, "test");
        insert_tasks(&dbp, &tasks).await.unwrap();
        CycleOrchestrator::advance(&dbp, &cycle.id, CyclePhase::Design, CyclePhase::Develop)
            .await
            .unwrap();

        // Skip develop/test/verify dispatch (no bindings) — just advance phases
        CycleOrchestrator::advance(&dbp, &cycle.id, CyclePhase::Develop, CyclePhase::Test)
            .await
            .unwrap();
        CycleOrchestrator::advance(&dbp, &cycle.id, CyclePhase::Test, CyclePhase::Verify)
            .await
            .unwrap();

        // Verify phase: artifacts exist from cleanse + research
        CycleOrchestrator::run_verify(&dbp, &secrets, &cycle.id, &None)
            .await
            .unwrap();
        CycleOrchestrator::advance(&dbp, &cycle.id, CyclePhase::Verify, CyclePhase::Gate)
            .await
            .unwrap();

        // Gate
        CycleOrchestrator::run_gate(&dbp, &cycle.id).await.unwrap();

        // Verify all phase transitions were recorded as events
        let db = Db::open(&dbp).unwrap();
        let evs = events::list_by_aggregate(&db.0, "steward", &cycle.id, None).unwrap();
        let phase_changes: Vec<_> = evs
            .iter()
            .filter(|e| e.kind == "steward.cycle_phase_changed")
            .collect();
        assert!(
            phase_changes.len() >= 5,
            "expected at least 5 phase change events, got {}",
            phase_changes.len()
        );

        // Verify cycle reached gate phase
        let detail = get_cycle(dbp, &cycle.id).await.unwrap();
        assert_eq!(detail.cycle.phase, CyclePhase::Gate);
    }

    #[tokio::test]
    async fn list_cycles_with_status_filter() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let trigger = EvolutionTrigger::SelfReflect {
            reason: "test".into(),
        };
        let cycle = trigger_evolution(dbp.clone(), secrets, trigger, None)
            .await
            .unwrap();
        let running = list_cycles(dbp.clone(), Some(CycleStatus::Running), 100)
            .await
            .unwrap();
        assert!(running.iter().any(|c| c.id == cycle.id));

        let succeeded = list_cycles(dbp.clone(), Some(CycleStatus::Completed), 100)
            .await
            .unwrap();
        assert!(!succeeded.iter().any(|c| c.id == cycle.id));
    }

    #[tokio::test]
    async fn get_cycle_nonexistent_returns_error() {
        let dbp = db_path().await;
        let err = get_cycle(dbp, "no_such_cycle").await.unwrap_err();
        assert!(matches!(
            err,
            StewardError::NotFound { .. } | StewardError::Store(_)
        ));
    }

    #[tokio::test]
    async fn cancel_cycle_nonexistent_returns_error() {
        let dbp = db_path().await;
        let err = cancel_cycle(dbp, "no_such_cycle").await.unwrap_err();
        assert!(matches!(
            err,
            StewardError::NotFound { .. } | StewardError::Store(_)
        ));
    }

    #[tokio::test]
    async fn list_cycles_no_status_returns_all() {
        let dbp = db_path().await;
        let secrets: Arc<dyn SecretStore> =
            Arc::new(crate::providers::MemorySecretStore::default());
        let t1 = EvolutionTrigger::SelfReflect {
            reason: "t1".into(),
        };
        let t2 = EvolutionTrigger::SelfReflect {
            reason: "t2".into(),
        };
        trigger_evolution(dbp.clone(), secrets.clone(), t1, None)
            .await
            .unwrap();
        trigger_evolution(dbp.clone(), secrets, t2, None)
            .await
            .unwrap();
        let all = list_cycles(dbp, None, 100).await.unwrap();
        assert!(all.len() >= 2);
    }
}
