//! Steward AI repositories — CRUD for the 10 tables introduced by migrations
//! 0027-0031. All SQL is parameterized; JSON columns use `serde_json::Value`.

use rusqlite::{params, OptionalExtension};

use crate::domain::steward_enums::{
    ArtifactStatus, ArtifactType, CyclePhase, CycleStatus, DevRoleKind, GateDecisionKind,
    TaskPhase, TaskStatus, TriggerSource,
};
use crate::domain::{
    AgentRefKind, DevRoleBinding, DevTeam, EvolutionArtifact, EvolutionCycle, EvolutionDataPool,
    EvolutionTask, GateDecision, StewardAi, StewardChangeSnapshot, StewardSession,
};
use crate::store::repos::sessions;
use crate::store::StoreError;

// ============================================================ steward_ai

/// Fetches the steward AI singleton row.
pub fn get_steward_ai(conn: &rusqlite::Connection) -> Result<Option<StewardAi>, StoreError> {
    conn.query_row(
        "SELECT id, ready, online_authorized, created_at FROM steward_ai LIMIT 1",
        [],
        |row| {
            let ready: i64 = row.get(1)?;
            let online: i64 = row.get(2)?;
            Ok(StewardAi {
                id: row.get(0)?,
                ready: ready != 0,
                online_authorized: online != 0,
                created_at: row.get(3)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// Upserts the steward AI singleton (INSERT OR REPLACE on PK id).
pub fn upsert_steward_ai(conn: &rusqlite::Connection, ai: &StewardAi) -> Result<(), StoreError> {
    conn.execute(
        "INSERT OR REPLACE INTO steward_ai (id, ready, online_authorized, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            ai.id,
            ai.ready as i64,
            ai.online_authorized as i64,
            ai.created_at
        ],
    )?;
    Ok(())
}

// ============================================================ steward_sessions

/// Inserts a steward session row. The corresponding `sessions` row must already
/// exist (same id, kind='background').
pub fn insert_steward_session(
    conn: &rusqlite::Connection,
    s: &StewardSession,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO steward_sessions (id, steward_id, title, goal, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            s.id,
            s.steward_id,
            s.title,
            s.goal,
            s.created_at,
            s.updated_at
        ],
    )?;
    Ok(())
}

/// Gets a single steward session by id (only if not soft-deleted in `sessions`).
pub fn get_steward_session(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<StewardSession, StoreError> {
    conn.query_row(
        "SELECT ss.id, ss.steward_id, ss.title, ss.goal, ss.created_at, ss.updated_at
         FROM steward_sessions ss
         JOIN sessions s ON s.id = ss.id
         WHERE ss.id = ?1 AND s.deleted_at IS NULL",
        params![id],
        row_to_steward_session,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "steward_session",
        id: id.to_string(),
    })
}

/// Lists steward sessions for a given steward id, most recently updated first,
/// excluding soft-deleted sessions.
pub fn list_steward_sessions(
    conn: &rusqlite::Connection,
    steward_id: &str,
    limit: u32,
) -> Result<Vec<StewardSession>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT ss.id, ss.steward_id, ss.title, ss.goal, ss.created_at, ss.updated_at
         FROM steward_sessions ss
         JOIN sessions s ON s.id = ss.id
         WHERE ss.steward_id = ?1 AND s.deleted_at IS NULL
         ORDER BY ss.updated_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![steward_id, limit], row_to_steward_session)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Updates a steward session's title and/or goal.
pub fn update_steward_session_title_goal(
    conn: &rusqlite::Connection,
    id: &str,
    title: Option<&str>,
    goal: Option<Option<&str>>,
) -> Result<(), StoreError> {
    let now = crate::domain::now_ms();
    let n = if let Some(t) = title {
        if let Some(g) = goal {
            conn.execute(
                "UPDATE steward_sessions SET title = ?2, goal = ?3, updated_at = ?4 WHERE id = ?1",
                params![id, t, g, now],
            )?
        } else {
            conn.execute(
                "UPDATE steward_sessions SET title = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, t, now],
            )?
        }
    } else if let Some(g) = goal {
        conn.execute(
            "UPDATE steward_sessions SET goal = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, g, now],
        )?
    } else {
        return Ok(());
    };
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "steward_session",
            id: id.to_string(),
        });
    }
    Ok(())
}

/// Soft-deletes a steward session by marking the underlying `sessions` row.
pub fn soft_delete_steward_session(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<(), StoreError> {
    sessions::delete(conn, id)
}

fn row_to_steward_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<StewardSession> {
    Ok(StewardSession {
        id: row.get(0)?,
        steward_id: row.get(1)?,
        title: row.get(2)?,
        goal: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

// ============================================================ steward_dev_team

/// Gets the dev team singleton.
pub fn get_dev_team(conn: &rusqlite::Connection) -> Result<Option<DevTeam>, StoreError> {
    conn.query_row(
        "SELECT id, steward_id, created_at FROM steward_dev_team LIMIT 1",
        [],
        |row| {
            Ok(DevTeam {
                id: row.get(0)?,
                steward_id: row.get(1)?,
                created_at: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

/// Upserts the dev team singleton.
pub fn upsert_dev_team(conn: &rusqlite::Connection, team: &DevTeam) -> Result<(), StoreError> {
    conn.execute(
        "INSERT OR REPLACE INTO steward_dev_team (id, steward_id, created_at)
         VALUES (?1, ?2, ?3)",
        params![team.id, team.steward_id, team.created_at],
    )?;
    Ok(())
}

// ============================================================ steward_dev_role_bindings

/// Gets all 5 dev role bindings.
pub fn get_dev_role_bindings_all(
    conn: &rusqlite::Connection,
) -> Result<Vec<DevRoleBinding>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT role_kind, agent_kind, agent_ref_id, updated_at
         FROM steward_dev_role_bindings ORDER BY role_kind ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        let role_kind_str: String = row.get(0)?;
        let agent_kind_str: String = row.get(1)?;
        let role_kind = DevRoleKind::parse(&role_kind_str).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("invalid role_kind: {role_kind_str}").into(),
            )
        })?;
        let agent_kind = AgentRefKind::parse(&agent_kind_str).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                format!("invalid agent_kind: {agent_kind_str}").into(),
            )
        })?;
        Ok(DevRoleBinding {
            role_kind,
            agent_kind,
            agent_ref_id: row.get(2)?,
            updated_at: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Upserts a single dev role binding by role_kind (PK). Users may replace but
/// never delete bindings.
pub fn upsert_dev_role_binding(
    conn: &rusqlite::Connection,
    binding: &DevRoleBinding,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT OR REPLACE INTO steward_dev_role_bindings
         (role_kind, agent_kind, agent_ref_id, updated_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            binding.role_kind.as_str(),
            binding.agent_kind.as_str(),
            binding.agent_ref_id,
            binding.updated_at,
        ],
    )?;
    Ok(())
}

// ============================================================ evolution_cycles

/// Inserts a new evolution cycle.
pub fn insert_cycle(conn: &rusqlite::Connection, c: &EvolutionCycle) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO evolution_cycles
         (id, trigger_source, trigger_context, phase, status, created_at, ended_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            c.id,
            c.trigger_source.as_str(),
            c.trigger_context,
            c.phase.as_str(),
            c.status.as_str(),
            c.created_at,
            c.ended_at,
        ],
    )?;
    Ok(())
}

/// Gets a single cycle by id.
pub fn get_cycle(conn: &rusqlite::Connection, id: &str) -> Result<EvolutionCycle, StoreError> {
    conn.query_row(
        "SELECT id, trigger_source, trigger_context, phase, status, created_at, ended_at
         FROM evolution_cycles WHERE id = ?1",
        params![id],
        row_to_cycle,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "evolution_cycle",
        id: id.to_string(),
    })
}

/// Lists cycles by status, most recently created first.
pub fn list_cycles_by_status(
    conn: &rusqlite::Connection,
    status: CycleStatus,
    limit: u32,
) -> Result<Vec<EvolutionCycle>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, trigger_source, trigger_context, phase, status, created_at, ended_at
         FROM evolution_cycles WHERE status = ?1 ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![status.as_str(), limit], row_to_cycle)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Lists all cycles, most recently created first.
pub fn list_cycles(
    conn: &rusqlite::Connection,
    limit: u32,
) -> Result<Vec<EvolutionCycle>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, trigger_source, trigger_context, phase, status, created_at, ended_at
         FROM evolution_cycles ORDER BY created_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], row_to_cycle)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Updates a cycle's phase and status. Callers must persist the
/// `steward.cycle_phase_changed` event BEFORE invoking this (iron rule).
pub fn update_cycle_phase_status(
    conn: &rusqlite::Connection,
    id: &str,
    phase: CyclePhase,
    status: CycleStatus,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE evolution_cycles SET phase = ?2, status = ?3 WHERE id = ?1",
        params![id, phase.as_str(), status.as_str()],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "evolution_cycle",
            id: id.to_string(),
        });
    }
    Ok(())
}

/// Marks a cycle as cancelled and sets ended_at. Does not touch tasks —
/// callers handle task cancellation separately.
pub fn cancel_cycle(
    conn: &rusqlite::Connection,
    id: &str,
    ended_at: i64,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE evolution_cycles SET status = 'cancelled', ended_at = ?2 WHERE id = ?1",
        params![id, ended_at],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "evolution_cycle",
            id: id.to_string(),
        });
    }
    Ok(())
}

fn row_to_cycle(row: &rusqlite::Row<'_>) -> rusqlite::Result<EvolutionCycle> {
    let trigger_str: String = row.get(1)?;
    let phase_str: String = row.get(3)?;
    let status_str: String = row.get(4)?;
    Ok(EvolutionCycle {
        id: row.get(0)?,
        trigger_source: TriggerSource::parse(&trigger_str).unwrap_or(TriggerSource::User),
        trigger_context: row.get(2)?,
        phase: CyclePhase::parse(&phase_str).unwrap_or(CyclePhase::Cleanse),
        status: CycleStatus::parse(&status_str).unwrap_or(CycleStatus::Running),
        created_at: row.get(5)?,
        ended_at: row.get(6)?,
    })
}

// ============================================================ evolution_tasks

/// Inserts a new evolution task.
pub fn insert_task(conn: &rusqlite::Connection, t: &EvolutionTask) -> Result<(), StoreError> {
    let depends_json = serde_json::to_string(&t.depends_on)?;
    conn.execute(
        "INSERT INTO evolution_tasks
         (id, cycle_id, phase, dev_role, depends_on_json, status, acceptance_criteria,
          trigger_source, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            t.id,
            t.cycle_id,
            t.phase.as_str(),
            t.dev_role.as_str(),
            depends_json,
            t.status.as_str(),
            t.acceptance_criteria,
            t.trigger_source,
            t.created_at,
            t.updated_at,
        ],
    )?;
    Ok(())
}

/// Lists all tasks for a cycle.
pub fn list_tasks_by_cycle(
    conn: &rusqlite::Connection,
    cycle_id: &str,
) -> Result<Vec<EvolutionTask>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, cycle_id, phase, dev_role, depends_on_json, status, acceptance_criteria,
                trigger_source, created_at, updated_at
         FROM evolution_tasks WHERE cycle_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![cycle_id], row_to_task)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Updates a task's status.
pub fn update_task_status(
    conn: &rusqlite::Connection,
    id: &str,
    status: TaskStatus,
) -> Result<(), StoreError> {
    let now = crate::domain::now_ms();
    let n = conn.execute(
        "UPDATE evolution_tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, status.as_str(), now],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "evolution_task",
            id: id.to_string(),
        });
    }
    Ok(())
}

/// Lists tasks that are ready to run: status='pending' AND all dependencies
/// have status='completed'.
pub fn list_ready_tasks(
    conn: &rusqlite::Connection,
    cycle_id: &str,
) -> Result<Vec<EvolutionTask>, StoreError> {
    let all_tasks = list_tasks_by_cycle(conn, cycle_id)?;
    let completed: std::collections::HashSet<String> = all_tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Completed)
        .map(|t| t.id.clone())
        .collect();
    let ready: Vec<EvolutionTask> = all_tasks
        .into_iter()
        .filter(|t| {
            t.status == TaskStatus::Pending
                && t.depends_on.iter().all(|dep| completed.contains(dep))
        })
        .collect();
    Ok(ready)
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<EvolutionTask> {
    let phase_str: String = row.get(2)?;
    let role_str: String = row.get(3)?;
    let depends_json: String = row.get(4)?;
    let status_str: String = row.get(5)?;
    let depends_on: Vec<String> = serde_json::from_str(&depends_json).unwrap_or_default();
    Ok(EvolutionTask {
        id: row.get(0)?,
        cycle_id: row.get(1)?,
        phase: TaskPhase::parse(&phase_str).unwrap_or(TaskPhase::Research),
        dev_role: DevRoleKind::parse(&role_str).unwrap_or(DevRoleKind::Researcher),
        depends_on,
        status: TaskStatus::parse(&status_str).unwrap_or(TaskStatus::Pending),
        acceptance_criteria: row.get(6)?,
        trigger_source: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

// ============================================================ evolution_artifacts

/// Inserts a new artifact.
pub fn insert_artifact(
    conn: &rusqlite::Connection,
    a: &EvolutionArtifact,
) -> Result<(), StoreError> {
    let content_json = serde_json::to_string(&a.content)?;
    let rollback_json = match &a.rollback_plan {
        Some(v) => Some(serde_json::to_string(v)?),
        None => None,
    };
    conn.execute(
        "INSERT INTO evolution_artifacts
         (id, task_id, produced_by_role, artifact_type, content_json, status,
          diff_preview, rollback_plan_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            a.id,
            a.task_id,
            a.produced_by_role,
            a.artifact_type.as_str(),
            content_json,
            a.status.as_str(),
            a.diff_preview,
            rollback_json,
            a.created_at,
        ],
    )?;
    Ok(())
}

/// Gets a single artifact by id.
pub fn get_artifact(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<EvolutionArtifact, StoreError> {
    conn.query_row(
        "SELECT id, task_id, produced_by_role, artifact_type, content_json, status,
                diff_preview, rollback_plan_json, created_at
         FROM evolution_artifacts WHERE id = ?1",
        params![id],
        row_to_artifact,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "evolution_artifact",
        id: id.to_string(),
    })
}

/// Lists artifacts for a task.
pub fn list_artifacts_by_task(
    conn: &rusqlite::Connection,
    task_id: &str,
) -> Result<Vec<EvolutionArtifact>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, produced_by_role, artifact_type, content_json, status,
                diff_preview, rollback_plan_json, created_at
         FROM evolution_artifacts WHERE task_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![task_id], row_to_artifact)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Lists artifacts by status (across all tasks), most recently created first.
pub fn list_artifacts_by_status(
    conn: &rusqlite::Connection,
    status: ArtifactStatus,
    limit: u32,
) -> Result<Vec<EvolutionArtifact>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, produced_by_role, artifact_type, content_json, status,
                diff_preview, rollback_plan_json, created_at
         FROM evolution_artifacts WHERE status = ?1 ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![status.as_str(), limit], row_to_artifact)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Lists artifacts for a cycle (via JOIN on tasks), most recently created first.
pub fn list_artifacts_by_cycle(
    conn: &rusqlite::Connection,
    cycle_id: &str,
) -> Result<Vec<EvolutionArtifact>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.task_id, a.produced_by_role, a.artifact_type, a.content_json,
                a.status, a.diff_preview, a.rollback_plan_json, a.created_at
         FROM evolution_artifacts a
         JOIN evolution_tasks t ON t.id = a.task_id
         WHERE t.cycle_id = ?1 ORDER BY a.created_at DESC",
    )?;
    let rows = stmt.query_map(params![cycle_id], row_to_artifact)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Updates an artifact's status.
pub fn update_artifact_status(
    conn: &rusqlite::Connection,
    id: &str,
    status: ArtifactStatus,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE evolution_artifacts SET status = ?2 WHERE id = ?1",
        params![id, status.as_str()],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "evolution_artifact",
            id: id.to_string(),
        });
    }
    Ok(())
}

/// Sets diff preview and rollback plan on an artifact.
pub fn set_artifact_diff_rollback(
    conn: &rusqlite::Connection,
    id: &str,
    diff_preview: Option<&str>,
    rollback_plan: Option<&serde_json::Value>,
) -> Result<(), StoreError> {
    let rollback_json = match rollback_plan {
        Some(v) => Some(serde_json::to_string(v)?),
        None => None,
    };
    let n = conn.execute(
        "UPDATE evolution_artifacts SET diff_preview = ?2, rollback_plan_json = ?3 WHERE id = ?1",
        params![id, diff_preview, rollback_json],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "evolution_artifact",
            id: id.to_string(),
        });
    }
    Ok(())
}

fn row_to_artifact(row: &rusqlite::Row<'_>) -> rusqlite::Result<EvolutionArtifact> {
    let type_str: String = row.get(3)?;
    let content_json: String = row.get(4)?;
    let status_str: String = row.get(5)?;
    let rollback_json: Option<String> = row.get(7)?;
    let content: serde_json::Value =
        serde_json::from_str(&content_json).unwrap_or(serde_json::Value::Null);
    let rollback_plan = rollback_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());
    Ok(EvolutionArtifact {
        id: row.get(0)?,
        task_id: row.get(1)?,
        produced_by_role: row.get(2)?,
        artifact_type: ArtifactType::parse(&type_str).unwrap_or(ArtifactType::ResearchReport),
        content,
        status: ArtifactStatus::parse(&status_str).unwrap_or(ArtifactStatus::PendingReview),
        diff_preview: row.get(6)?,
        rollback_plan,
        created_at: row.get(8)?,
    })
}

// ============================================================ evolution_data_pools

/// Inserts a new data pool.
pub fn insert_data_pool(
    conn: &rusqlite::Connection,
    p: &EvolutionDataPool,
) -> Result<(), StoreError> {
    let scope_json = serde_json::to_string(&p.scope)?;
    let product_json = serde_json::to_string(&p.product)?;
    conn.execute(
        "INSERT INTO evolution_data_pools (id, scope_json, rules_id, product_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![p.id, scope_json, p.rules_id, product_json, p.created_at],
    )?;
    Ok(())
}

/// Lists data pools, most recently created first.
pub fn list_data_pools(
    conn: &rusqlite::Connection,
    limit: u32,
) -> Result<Vec<EvolutionDataPool>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, scope_json, rules_id, product_json, created_at
         FROM evolution_data_pools ORDER BY created_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], row_to_data_pool)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Gets a single data pool by id.
pub fn get_data_pool(
    conn: &rusqlite::Connection,
    id: &str,
) -> Result<EvolutionDataPool, StoreError> {
    conn.query_row(
        "SELECT id, scope_json, rules_id, product_json, created_at
         FROM evolution_data_pools WHERE id = ?1",
        params![id],
        row_to_data_pool,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "evolution_data_pool",
        id: id.to_string(),
    })
}

fn row_to_data_pool(row: &rusqlite::Row<'_>) -> rusqlite::Result<EvolutionDataPool> {
    let scope_json: String = row.get(1)?;
    let product_json: String = row.get(3)?;
    let scope: serde_json::Value =
        serde_json::from_str(&scope_json).unwrap_or(serde_json::Value::Null);
    let product: serde_json::Value =
        serde_json::from_str(&product_json).unwrap_or(serde_json::Value::Null);
    Ok(EvolutionDataPool {
        id: row.get(0)?,
        scope,
        rules_id: row.get(2)?,
        product,
        created_at: row.get(4)?,
    })
}

// ============================================================ evolution_gate_decisions

/// Inserts a gate decision. Fails with UNIQUE constraint if a decision already
/// exists for the artifact.
pub fn insert_gate_decision(
    conn: &rusqlite::Connection,
    d: &GateDecision,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO evolution_gate_decisions (id, artifact_id, decision, reason, decided_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            d.id,
            d.artifact_id,
            d.decision.as_str(),
            d.reason,
            d.decided_at
        ],
    )?;
    Ok(())
}

/// Gets the gate decision for an artifact (at most one, by UNIQUE constraint).
pub fn get_gate_decision_by_artifact(
    conn: &rusqlite::Connection,
    artifact_id: &str,
) -> Result<Option<GateDecision>, StoreError> {
    conn.query_row(
        "SELECT id, artifact_id, decision, reason, decided_at
         FROM evolution_gate_decisions WHERE artifact_id = ?1",
        params![artifact_id],
        |row| {
            let decision_str: String = row.get(2)?;
            Ok(GateDecision {
                id: row.get(0)?,
                artifact_id: row.get(1)?,
                decision: GateDecisionKind::parse(&decision_str)
                    .unwrap_or(GateDecisionKind::Reject),
                reason: row.get(3)?,
                decided_at: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(Into::into)
}

// ============================================================ steward_change_snapshots

/// Inserts a change snapshot for rollback.
pub fn insert_change_snapshot(
    conn: &rusqlite::Connection,
    s: &StewardChangeSnapshot,
) -> Result<(), StoreError> {
    let before_json = serde_json::to_string(&s.before)?;
    let after_json = serde_json::to_string(&s.after)?;
    conn.execute(
        "INSERT INTO steward_change_snapshots
         (id, proposal_id, target_type, target_id, before_json, after_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            s.id,
            s.proposal_id,
            s.target_type,
            s.target_id,
            before_json,
            after_json,
            s.created_at
        ],
    )?;
    Ok(())
}

/// Gets all change snapshots for a proposal (there may be multiple if the
/// proposal touches several targets).
pub fn get_change_snapshots_by_proposal(
    conn: &rusqlite::Connection,
    proposal_id: &str,
) -> Result<Vec<StewardChangeSnapshot>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, proposal_id, target_type, target_id, before_json, after_json, created_at
         FROM steward_change_snapshots WHERE proposal_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![proposal_id], row_to_change_snapshot)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Gets a single change snapshot by its id (for rollback).
pub fn get_change_snapshot(
    conn: &rusqlite::Connection,
    snapshot_id: &str,
) -> Result<StewardChangeSnapshot, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, proposal_id, target_type, target_id, before_json, after_json, created_at
         FROM steward_change_snapshots WHERE id = ?1",
    )?;
    stmt.query_row(params![snapshot_id], row_to_change_snapshot)
        .map_err(Into::into)
}

fn row_to_change_snapshot(row: &rusqlite::Row<'_>) -> rusqlite::Result<StewardChangeSnapshot> {
    let before_json: String = row.get(4)?;
    let after_json: String = row.get(5)?;
    let before: serde_json::Value =
        serde_json::from_str(&before_json).unwrap_or(serde_json::Value::Null);
    let after: serde_json::Value =
        serde_json::from_str(&after_json).unwrap_or(serde_json::Value::Null);
    Ok(StewardChangeSnapshot {
        id: row.get(0)?,
        proposal_id: row.get(1)?,
        target_type: row.get(2)?,
        target_id: row.get(3)?,
        before,
        after,
        created_at: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    fn db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    /// Inserts a minimal sessions row (kind='background') for FK support.
    fn ensure_session(conn: &rusqlite::Connection, id: &str) {
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at, kind)
             VALUES (?1, 'test', 1, 1, 'background')",
            params![id],
        )
        .unwrap();
    }

    fn ensure_steward_ai(conn: &rusqlite::Connection, id: &str) -> StewardAi {
        let ai = StewardAi {
            id: id.into(),
            ready: true,
            online_authorized: false,
            created_at: 1,
        };
        upsert_steward_ai(conn, &ai).unwrap();
        ai
    }

    fn ensure_cycle(conn: &rusqlite::Connection, id: &str) -> EvolutionCycle {
        let c = EvolutionCycle {
            id: id.into(),
            trigger_source: TriggerSource::User,
            trigger_context: "ctx".into(),
            phase: CyclePhase::Cleanse,
            status: CycleStatus::Running,
            created_at: 1,
            ended_at: None,
        };
        insert_cycle(conn, &c).unwrap();
        c
    }

    fn ensure_task(conn: &rusqlite::Connection, id: &str, cycle_id: &str) -> EvolutionTask {
        let t = EvolutionTask {
            id: id.into(),
            cycle_id: cycle_id.into(),
            phase: TaskPhase::Research,
            dev_role: DevRoleKind::Researcher,
            depends_on: vec![],
            status: TaskStatus::Pending,
            acceptance_criteria: "must pass".into(),
            trigger_source: "user".into(),
            created_at: 1,
            updated_at: 1,
        };
        insert_task(conn, &t).unwrap();
        t
    }

    fn ensure_artifact(conn: &rusqlite::Connection, id: &str, task_id: &str) -> EvolutionArtifact {
        let a = EvolutionArtifact {
            id: id.into(),
            task_id: task_id.into(),
            produced_by_role: "researcher".into(),
            artifact_type: ArtifactType::ResearchReport,
            content: serde_json::json!({"summary": "ok"}),
            status: ArtifactStatus::PendingReview,
            diff_preview: None,
            rollback_plan: None,
            created_at: 1,
        };
        insert_artifact(conn, &a).unwrap();
        a
    }

    // ---------------------------------------------------- steward_ai

    #[test]
    fn steward_ai_upsert_get_roundtrip() {
        let conn = db();
        let ai = ensure_steward_ai(&conn, "s1");
        let got = get_steward_ai(&conn).unwrap().unwrap();
        assert_eq!(got.id, ai.id);
        assert!(got.ready);
        assert!(!got.online_authorized);
    }

    #[test]
    fn steward_ai_upsert_replaces() {
        let conn = db();
        ensure_steward_ai(&conn, "s1");
        let updated = StewardAi {
            id: "s1".into(),
            ready: false,
            online_authorized: true,
            created_at: 2,
        };
        upsert_steward_ai(&conn, &updated).unwrap();
        let got = get_steward_ai(&conn).unwrap().unwrap();
        assert!(!got.ready);
        assert!(got.online_authorized);
    }

    #[test]
    fn steward_ai_get_none_when_empty() {
        let conn = db();
        assert!(get_steward_ai(&conn).unwrap().is_none());
    }

    // ---------------------------------------------------- steward_sessions

    #[test]
    fn steward_session_insert_get_roundtrip() {
        let conn = db();
        ensure_session(&conn, "sess1");
        let s = StewardSession {
            id: "sess1".into(),
            steward_id: "s1".into(),
            title: "t".into(),
            goal: Some("g".into()),
            created_at: 1,
            updated_at: 2,
        };
        insert_steward_session(&conn, &s).unwrap();
        let got = get_steward_session(&conn, "sess1").unwrap();
        assert_eq!(got.id, "sess1");
        assert_eq!(got.title, "t");
        assert_eq!(got.goal.as_deref(), Some("g"));
    }

    #[test]
    fn steward_session_list_by_steward() {
        let conn = db();
        ensure_session(&conn, "s1");
        ensure_session(&conn, "s2");
        insert_steward_session(
            &conn,
            &StewardSession {
                id: "s1".into(),
                steward_id: "st".into(),
                title: "a".into(),
                goal: None,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        insert_steward_session(
            &conn,
            &StewardSession {
                id: "s2".into(),
                steward_id: "st".into(),
                title: "b".into(),
                goal: None,
                created_at: 2,
                updated_at: 2,
            },
        )
        .unwrap();
        let list = list_steward_sessions(&conn, "st", 10).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, "s2");
    }

    #[test]
    fn steward_session_soft_delete_hides_from_get() {
        let conn = db();
        ensure_session(&conn, "sess1");
        insert_steward_session(
            &conn,
            &StewardSession {
                id: "sess1".into(),
                steward_id: "s1".into(),
                title: "t".into(),
                goal: None,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        soft_delete_steward_session(&conn, "sess1").unwrap();
        assert!(get_steward_session(&conn, "sess1").is_err());
    }

    #[test]
    fn steward_session_fk_cascade_on_session_delete() {
        let conn = db();
        ensure_session(&conn, "sess1");
        insert_steward_session(
            &conn,
            &StewardSession {
                id: "sess1".into(),
                steward_id: "s1".into(),
                title: "t".into(),
                goal: None,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        conn.execute("DELETE FROM sessions WHERE id = 'sess1'", [])
            .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM steward_sessions WHERE id = 'sess1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "steward_sessions row should cascade-delete");
    }

    // ---------------------------------------------------- steward_dev_team

    #[test]
    fn dev_team_upsert_get_roundtrip() {
        let conn = db();
        ensure_steward_ai(&conn, "s1");
        let team = DevTeam {
            id: "steward_dev_team".into(),
            steward_id: "s1".into(),
            created_at: 1,
        };
        upsert_dev_team(&conn, &team).unwrap();
        let got = get_dev_team(&conn).unwrap().unwrap();
        assert_eq!(got.id, "steward_dev_team");
    }

    // ---------------------------------------------------- dev_role_bindings

    #[test]
    fn dev_role_binding_upsert_get_all() {
        let conn = db();
        let b = DevRoleBinding {
            role_kind: DevRoleKind::Researcher,
            agent_kind: AgentRefKind::Role,
            agent_ref_id: "role_x".into(),
            updated_at: 1,
        };
        upsert_dev_role_binding(&conn, &b).unwrap();
        let all = get_dev_role_bindings_all(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].role_kind, DevRoleKind::Researcher);
    }

    #[test]
    fn dev_role_binding_upsert_replaces() {
        let conn = db();
        upsert_dev_role_binding(
            &conn,
            &DevRoleBinding {
                role_kind: DevRoleKind::Designer,
                agent_kind: AgentRefKind::Cli,
                agent_ref_id: "old".into(),
                updated_at: 1,
            },
        )
        .unwrap();
        upsert_dev_role_binding(
            &conn,
            &DevRoleBinding {
                role_kind: DevRoleKind::Designer,
                agent_kind: AgentRefKind::Role,
                agent_ref_id: "new".into(),
                updated_at: 2,
            },
        )
        .unwrap();
        let all = get_dev_role_bindings_all(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].agent_ref_id, "new");
    }

    // ---------------------------------------------------- evolution_cycles

    #[test]
    fn cycle_insert_get_roundtrip() {
        let conn = db();
        ensure_cycle(&conn, "c1");
        let got = get_cycle(&conn, "c1").unwrap();
        assert_eq!(got.phase, CyclePhase::Cleanse);
        assert_eq!(got.status, CycleStatus::Running);
    }

    #[test]
    fn cycle_update_phase_status() {
        let conn = db();
        ensure_cycle(&conn, "c1");
        update_cycle_phase_status(&conn, "c1", CyclePhase::Research, CycleStatus::Running).unwrap();
        let got = get_cycle(&conn, "c1").unwrap();
        assert_eq!(got.phase, CyclePhase::Research);
    }

    #[test]
    fn cycle_cancel_sets_status_and_ended() {
        let conn = db();
        ensure_cycle(&conn, "c1");
        cancel_cycle(&conn, "c1", 99).unwrap();
        let got = get_cycle(&conn, "c1").unwrap();
        assert_eq!(got.status, CycleStatus::Cancelled);
        assert_eq!(got.ended_at, Some(99));
    }

    #[test]
    fn cycle_list_by_status() {
        let conn = db();
        ensure_cycle(&conn, "c1");
        let running = list_cycles_by_status(&conn, CycleStatus::Running, 10).unwrap();
        assert_eq!(running.len(), 1);
    }

    // ---------------------------------------------------- evolution_tasks

    #[test]
    fn task_insert_list_by_cycle() {
        let conn = db();
        ensure_cycle(&conn, "c1");
        ensure_task(&conn, "t1", "c1");
        let tasks = list_tasks_by_cycle(&conn, "c1").unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "t1");
    }

    #[test]
    fn task_update_status() {
        let conn = db();
        ensure_cycle(&conn, "c1");
        ensure_task(&conn, "t1", "c1");
        update_task_status(&conn, "t1", TaskStatus::Running).unwrap();
        let tasks = list_tasks_by_cycle(&conn, "c1").unwrap();
        assert_eq!(tasks[0].status, TaskStatus::Running);
    }

    #[test]
    fn task_list_ready_respects_dependencies() {
        let conn = db();
        ensure_cycle(&conn, "c1");
        // t1 has no deps, t2 depends on t1
        insert_task(
            &conn,
            &EvolutionTask {
                id: "t1".into(),
                cycle_id: "c1".into(),
                phase: TaskPhase::Research,
                dev_role: DevRoleKind::Researcher,
                depends_on: vec![],
                status: TaskStatus::Pending,
                acceptance_criteria: "ok".into(),
                trigger_source: "user".into(),
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();
        insert_task(
            &conn,
            &EvolutionTask {
                id: "t2".into(),
                cycle_id: "c1".into(),
                phase: TaskPhase::Design,
                dev_role: DevRoleKind::Designer,
                depends_on: vec!["t1".into()],
                status: TaskStatus::Pending,
                acceptance_criteria: "ok".into(),
                trigger_source: "user".into(),
                created_at: 2,
                updated_at: 2,
            },
        )
        .unwrap();
        // Only t1 is ready (t2 depends on t1 which is not completed)
        let ready = list_ready_tasks(&conn, "c1").unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "t1");
        // Complete t1 → t2 becomes ready
        update_task_status(&conn, "t1", TaskStatus::Completed).unwrap();
        let ready = list_ready_tasks(&conn, "c1").unwrap();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].id, "t2");
    }

    // ---------------------------------------------------- evolution_artifacts

    #[test]
    fn artifact_insert_get_roundtrip() {
        let conn = db();
        ensure_cycle(&conn, "c1");
        ensure_task(&conn, "t1", "c1");
        ensure_artifact(&conn, "a1", "t1");
        let got = get_artifact(&conn, "a1").unwrap();
        assert_eq!(got.artifact_type, ArtifactType::ResearchReport);
        assert_eq!(got.content["summary"], "ok");
    }

    #[test]
    fn artifact_list_by_task_and_status() {
        let conn = db();
        ensure_cycle(&conn, "c1");
        ensure_task(&conn, "t1", "c1");
        ensure_artifact(&conn, "a1", "t1");
        let by_task = list_artifacts_by_task(&conn, "t1").unwrap();
        assert_eq!(by_task.len(), 1);
        let by_status = list_artifacts_by_status(&conn, ArtifactStatus::PendingReview, 10).unwrap();
        assert_eq!(by_status.len(), 1);
    }

    #[test]
    fn artifact_update_status_and_diff_rollback() {
        let conn = db();
        ensure_cycle(&conn, "c1");
        ensure_task(&conn, "t1", "c1");
        ensure_artifact(&conn, "a1", "t1");
        update_artifact_status(&conn, "a1", ArtifactStatus::Approved).unwrap();
        let rollback = serde_json::json!({"type": "prompt", "version": "v1"});
        set_artifact_diff_rollback(&conn, "a1", Some("diff text"), Some(&rollback)).unwrap();
        let got = get_artifact(&conn, "a1").unwrap();
        assert_eq!(got.status, ArtifactStatus::Approved);
        assert_eq!(got.diff_preview.as_deref(), Some("diff text"));
        assert!(got.rollback_plan.is_some());
    }

    #[test]
    fn artifact_list_by_cycle() {
        let conn = db();
        ensure_cycle(&conn, "c1");
        ensure_task(&conn, "t1", "c1");
        ensure_artifact(&conn, "a1", "t1");
        let by_cycle = list_artifacts_by_cycle(&conn, "c1").unwrap();
        assert_eq!(by_cycle.len(), 1);
    }

    // ---------------------------------------------------- evolution_data_pools

    #[test]
    fn data_pool_insert_list_get_roundtrip() {
        let conn = db();
        let p = EvolutionDataPool {
            id: "p1".into(),
            scope: serde_json::json!({"range": "7d"}),
            rules_id: "r1".into(),
            product: serde_json::json!({"patterns": []}),
            created_at: 1,
        };
        insert_data_pool(&conn, &p).unwrap();
        let got = get_data_pool(&conn, "p1").unwrap();
        assert_eq!(got.rules_id, "r1");
        assert_eq!(got.scope["range"], "7d");
        let list = list_data_pools(&conn, 10).unwrap();
        assert_eq!(list.len(), 1);
    }

    // ---------------------------------------------------- evolution_gate_decisions

    #[test]
    fn gate_decision_insert_get_by_artifact() {
        let conn = db();
        ensure_cycle(&conn, "c1");
        ensure_task(&conn, "t1", "c1");
        ensure_artifact(&conn, "a1", "t1");
        let d = GateDecision {
            id: "d1".into(),
            artifact_id: "a1".into(),
            decision: GateDecisionKind::Approve,
            reason: Some("looks good".into()),
            decided_at: 1,
        };
        insert_gate_decision(&conn, &d).unwrap();
        let got = get_gate_decision_by_artifact(&conn, "a1").unwrap().unwrap();
        assert_eq!(got.decision, GateDecisionKind::Approve);
    }

    #[test]
    fn gate_decision_unique_constraint_on_artifact() {
        let conn = db();
        ensure_cycle(&conn, "c1");
        ensure_task(&conn, "t1", "c1");
        ensure_artifact(&conn, "a1", "t1");
        insert_gate_decision(
            &conn,
            &GateDecision {
                id: "d1".into(),
                artifact_id: "a1".into(),
                decision: GateDecisionKind::Approve,
                reason: None,
                decided_at: 1,
            },
        )
        .unwrap();
        // Second decision for same artifact must fail (UNIQUE constraint)
        let result = insert_gate_decision(
            &conn,
            &GateDecision {
                id: "d2".into(),
                artifact_id: "a1".into(),
                decision: GateDecisionKind::Reject,
                reason: None,
                decided_at: 2,
            },
        );
        assert!(result.is_err(), "UNIQUE(artifact_id) must reject duplicate");
    }

    // ---------------------------------------------------- steward_change_snapshots

    #[test]
    fn change_snapshot_insert_get_by_proposal() {
        let conn = db();
        ensure_cycle(&conn, "c1");
        ensure_task(&conn, "t1", "c1");
        ensure_artifact(&conn, "a1", "t1");
        let s = StewardChangeSnapshot {
            id: "snap1".into(),
            proposal_id: "a1".into(),
            target_type: "role".into(),
            target_id: "r1".into(),
            before: serde_json::json!({"name": "old"}),
            after: serde_json::json!({"name": "new"}),
            created_at: 1,
        };
        insert_change_snapshot(&conn, &s).unwrap();
        let snaps = get_change_snapshots_by_proposal(&conn, "a1").unwrap();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].before["name"], "old");
        assert_eq!(snaps[0].after["name"], "new");
    }

    // ---------------------------------------------------- builtin roles (migration 0031)

    #[test]
    fn builtin_steward_roles_seeded() {
        let conn = db();
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM roles WHERE id LIKE 'role_steward_%' AND builtin = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 5, "5 builtin steward roles must be seeded");
    }
}
