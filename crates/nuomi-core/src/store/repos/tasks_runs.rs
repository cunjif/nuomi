//! Repositories for the UI M1 execution domain: tasks, runs, approvals,
//! schedules (migration 0002). CRUD + optimistic status guards only —
//! transition legality is the caller's job (domain::run_state).

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{
    AgentRefKind, Approval, ApprovalDecision, Run, RunState, Schedule, ScheduleSessionMode,
    ScheduleTargetKind, Task, TaskStatus,
};
use crate::store::StoreError;

// ---------------------------------------------------------------- tasks

pub fn insert_task(conn: &Connection, t: &Task) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO tasks (id, session_id, title, description, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            t.id,
            t.session_id,
            t.title,
            t.description,
            t.status.as_str(),
            t.created_at,
            t.updated_at
        ],
    )?;
    Ok(())
}

pub fn get_task(conn: &Connection, id: &str) -> Result<Task, StoreError> {
    conn.query_row(
        "SELECT id, session_id, title, description, status, created_at, updated_at
         FROM tasks WHERE id = ?1",
        params![id],
        row_to_task,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "task",
        id: id.to_string(),
    })
}

/// Optimistic guard: updates only when the current status still equals
/// `expected`. Zero affected rows → `NotFound` or `Conflict`.
pub fn update_task_status(
    conn: &Connection,
    id: &str,
    expected: TaskStatus,
    next: TaskStatus,
    at: i64,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE tasks SET status = ?2, updated_at = ?3 WHERE id = ?1 AND status = ?4",
        params![id, next.as_str(), at, expected.as_str()],
    )?;
    guard_rows(n, "task", id, expected.as_str())
}

pub fn list_tasks(conn: &Connection, limit: u32) -> Result<Vec<Task>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, title, description, status, created_at, updated_at
         FROM tasks ORDER BY created_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], row_to_task)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn list_tasks_by_status(
    conn: &Connection,
    status: TaskStatus,
    limit: u32,
) -> Result<Vec<Task>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, title, description, status, created_at, updated_at
         FROM tasks WHERE status = ?1 ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![status.as_str(), limit], row_to_task)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Removes the task row itself; `Ok(false)` when no row matched. Children
/// are NOT touched here — runs/approvals must be cleared first (the 0002
/// FKs have no CASCADE); prefer [`delete_cascade`].
pub fn delete(conn: &Connection, id: &str) -> Result<bool, StoreError> {
    let n = conn.execute("DELETE FROM tasks WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

/// Atomic task deletion: approvals → runs → task, all inside ONE
/// transaction so the FK chain can never observe an intermediate state.
/// `events` is intentionally left untouched — the log is append-only and
/// dangling aggregates are acceptable history. `Ok(false)` when the task
/// did not exist (nothing was written).
pub fn delete_cascade(conn: &Connection, id: &str) -> Result<bool, StoreError> {
    let tx = conn.unchecked_transaction()?;
    delete_approvals_for_task(&tx, id)?;
    delete_runs_for_task(&tx, id)?;
    let deleted = delete(&tx, id)?;
    tx.commit()?;
    Ok(deleted)
}

// ---------------------------------------------------------------- runs

pub fn insert_run(conn: &Connection, r: &Run) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO runs (id, task_id, session_id, status, heartbeat_at, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            r.id,
            r.task_id,
            r.session_id,
            r.status.as_str(),
            r.heartbeat_at,
            r.created_at,
            r.updated_at
        ],
    )?;
    Ok(())
}

pub fn get_run(conn: &Connection, id: &str) -> Result<Run, StoreError> {
    conn.query_row(
        "SELECT id, task_id, session_id, status, heartbeat_at, created_at, updated_at
         FROM runs WHERE id = ?1",
        params![id],
        row_to_run,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "run",
        id: id.to_string(),
    })
}

/// Optimistic status guard around [`RunState`] transitions.
pub fn update_run_status(
    conn: &Connection,
    id: &str,
    expected: RunState,
    next: RunState,
    at: i64,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE runs SET status = ?2, updated_at = ?3 WHERE id = ?1 AND status = ?4",
        params![id, next.as_str(), at, expected.as_str()],
    )?;
    guard_rows(n, "run", id, expected.as_str())
}

/// Heartbeat touch (running-run liveness; orphan detection reads this).
pub fn heartbeat(conn: &Connection, id: &str, at: i64) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE runs SET heartbeat_at = ?2, updated_at = ?2 WHERE id = ?1",
        params![id, at],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "run",
            id: id.to_string(),
        });
    }
    Ok(())
}

pub fn list_runs_by_task(conn: &Connection, task_id: &str) -> Result<Vec<Run>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, session_id, status, heartbeat_at, created_at, updated_at
         FROM runs WHERE task_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![task_id], row_to_run)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn list_runs_by_status(conn: &Connection, status: RunState) -> Result<Vec<Run>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, session_id, status, heartbeat_at, created_at, updated_at
         FROM runs WHERE status = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![status.as_str()], row_to_run)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Lists active runs (running or queued or awaiting_approval) for a session.
pub fn list_active_runs_by_session(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<Run>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, session_id, status, heartbeat_at, created_at, updated_at
         FROM runs
         WHERE session_id = ?1 AND status IN ('running', 'queued', 'awaiting_approval')
         ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![session_id], row_to_run)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Removes every run of a task; returns how many rows went away. Approvals
/// referencing those runs must be deleted first (see
/// [`delete_approvals_for_task`]).
pub fn delete_runs_for_task(conn: &Connection, task_id: &str) -> Result<usize, StoreError> {
    let n = conn.execute("DELETE FROM runs WHERE task_id = ?1", params![task_id])?;
    Ok(n)
}

// ---------------------------------------------------------------- approvals

pub fn insert_approval(conn: &Connection, a: &Approval) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO approvals (id, run_id, tool_name, arguments_json, decision, decided_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            a.id,
            a.run_id,
            a.tool_name,
            a.arguments_json,
            a.decision.as_str(),
            a.decided_at,
            a.created_at
        ],
    )?;
    Ok(())
}

pub fn get_approval(conn: &Connection, id: &str) -> Result<Approval, StoreError> {
    conn.query_row(
        "SELECT id, run_id, tool_name, arguments_json, decision, decided_at, created_at
         FROM approvals WHERE id = ?1",
        params![id],
        row_to_approval,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "approval",
        id: id.to_string(),
    })
}

/// Resolves a pending approval; guarded so a decision can never be flipped.
pub fn resolve_approval(
    conn: &Connection,
    id: &str,
    decision: ApprovalDecision,
    decided_at: i64,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE approvals SET decision = ?2, decided_at = ?3 WHERE id = ?1 AND decision = 'pending'",
        params![id, decision.as_str(), decided_at],
    )?;
    guard_rows(n, "approval", id, ApprovalDecision::Pending.as_str())
}

pub fn list_pending_approvals(conn: &Connection) -> Result<Vec<Approval>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, run_id, tool_name, arguments_json, decision, decided_at, created_at
         FROM approvals WHERE decision = 'pending' ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], row_to_approval)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn list_approvals_by_run(conn: &Connection, run_id: &str) -> Result<Vec<Approval>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, run_id, tool_name, arguments_json, decision, decided_at, created_at
         FROM approvals WHERE run_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![run_id], row_to_approval)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Removes approvals hanging off a task's runs (children before the parent
/// runs); returns how many rows went away.
pub fn delete_approvals_for_task(conn: &Connection, task_id: &str) -> Result<usize, StoreError> {
    let n = conn.execute(
        "DELETE FROM approvals WHERE run_id IN (SELECT id FROM runs WHERE task_id = ?1)",
        params![task_id],
    )?;
    Ok(n)
}

// ---------------------------------------------------------------- schedules

pub fn insert_schedule(conn: &Connection, s: &Schedule) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO schedules
         (id, name, cron_expr, task_title, task_description, enabled,
          last_triggered_at, next_trigger_at, created_at, updated_at,
          target_kind, agent_kind, agent_ref_id, team_id, session_mode, session_id, auto_dispatch)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            s.id,
            s.name,
            s.cron_expr,
            s.task_title,
            s.task_description,
            s.enabled as i64,
            s.last_triggered_at,
            s.next_trigger_at,
            s.created_at,
            s.updated_at,
            s.target_kind.as_str(),
            s.agent.as_ref().map(|(k, _)| k.as_str()),
            s.agent.as_ref().map(|(_, id)| id.as_str()),
            s.team_id,
            s.session_mode.as_str(),
            s.session_id,
            s.auto_dispatch as i64,
        ],
    )?;
    Ok(())
}

pub fn get_schedule(conn: &Connection, id: &str) -> Result<Schedule, StoreError> {
    conn.query_row(
        &schedule_select("WHERE id = ?1"),
        params![id],
        row_to_schedule,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "schedule",
        id: id.to_string(),
    })
}

pub fn get_schedule_by_name(conn: &Connection, name: &str) -> Result<Schedule, StoreError> {
    conn.query_row(
        &schedule_select("WHERE name = ?1"),
        params![name],
        row_to_schedule,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "schedule",
        id: name.to_string(),
    })
}

/// Full-row update of mutable fields (management page edits).
pub fn update_schedule(conn: &Connection, s: &Schedule) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE schedules SET name = ?2, cron_expr = ?3, task_title = ?4, task_description = ?5,
         enabled = ?6, last_triggered_at = ?7, next_trigger_at = ?8, updated_at = ?9,
         target_kind = ?10, agent_kind = ?11, agent_ref_id = ?12, team_id = ?13,
         session_mode = ?14, session_id = ?15, auto_dispatch = ?16
         WHERE id = ?1",
        params![
            s.id,
            s.name,
            s.cron_expr,
            s.task_title,
            s.task_description,
            s.enabled as i64,
            s.last_triggered_at,
            s.next_trigger_at,
            s.updated_at,
            s.target_kind.as_str(),
            s.agent.as_ref().map(|(k, _)| k.as_str()),
            s.agent.as_ref().map(|(_, id)| id.as_str()),
            s.team_id,
            s.session_mode.as_str(),
            s.session_id,
            s.auto_dispatch as i64,
        ],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "schedule",
            id: s.id.clone(),
        });
    }
    Ok(())
}

pub fn set_schedule_enabled(
    conn: &Connection,
    id: &str,
    enabled: bool,
    at: i64,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE schedules SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, enabled as i64, at],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "schedule",
            id: id.to_string(),
        });
    }
    Ok(())
}

/// All enabled schedules whose `next_trigger_at` has passed.
pub fn due_schedules(conn: &Connection, now: i64) -> Result<Vec<Schedule>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "{} WHERE enabled = 1 AND next_trigger_at IS NOT NULL AND next_trigger_at <= ?1
         ORDER BY next_trigger_at ASC",
        schedule_select("")
    ))?;
    let rows = stmt.query_map(params![now], row_to_schedule)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Advances trigger bookkeeping after a fire.
pub fn mark_schedule_triggered(
    conn: &Connection,
    id: &str,
    triggered_at: i64,
    next_trigger_at: Option<i64>,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE schedules SET last_triggered_at = ?2, next_trigger_at = ?3, updated_at = ?2
         WHERE id = ?1",
        params![id, triggered_at, next_trigger_at],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "schedule",
            id: id.to_string(),
        });
    }
    Ok(())
}

pub fn list_schedules(conn: &Connection, limit: u32) -> Result<Vec<Schedule>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "{} ORDER BY created_at DESC LIMIT ?1",
        schedule_select("")
    ))?;
    let rows = stmt.query_map(params![limit], row_to_schedule)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

// ---------------------------------------------------------------- workspace-scoped

/// Lists tasks belonging to a specific workspace, ordered by `created_at DESC`.
pub fn list_tasks_by_workspace(
    conn: &Connection,
    workspace_id: &str,
    limit: u32,
) -> Result<Vec<Task>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, title, description, status, created_at, updated_at
         FROM tasks WHERE workspace_id = ?1 ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![workspace_id, limit], row_to_task)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Lists runs belonging to a specific workspace, ordered by `created_at ASC`.
pub fn list_runs_by_workspace(
    conn: &Connection,
    workspace_id: &str,
) -> Result<Vec<Run>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, session_id, status, heartbeat_at, created_at, updated_at
         FROM runs WHERE workspace_id = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![workspace_id], row_to_run)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Lists active runs (running/queued/awaiting_approval) for a workspace.
pub fn list_active_runs_by_workspace(
    conn: &Connection,
    workspace_id: &str,
) -> Result<Vec<Run>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, session_id, status, heartbeat_at, created_at, updated_at
         FROM runs
         WHERE workspace_id = ?1 AND status IN ('running', 'queued', 'awaiting_approval')
         ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![workspace_id], row_to_run)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Counts "unread" task completions/failures and pending approvals for a
/// workspace since the given timestamp (typically `last_focused_at`). Used
/// for the unread indicator on non-focused workspace tabs.
pub fn count_unread_since(
    conn: &Connection,
    workspace_id: &str,
    since: i64,
) -> Result<i64, StoreError> {
    let task_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks
         WHERE workspace_id = ?1 AND updated_at > ?2
         AND status IN ('done', 'failed')",
        params![workspace_id, since],
        |row| row.get(0),
    )?;
    let run_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM runs
         WHERE workspace_id = ?1 AND updated_at > ?2
         AND status = 'awaiting_approval'",
        params![workspace_id, since],
        |row| row.get(0),
    )?;
    Ok(task_count + run_count)
}

/// Binds a task to a workspace (sets `workspace_id` on an existing task row).
pub fn bind_task_workspace(
    conn: &Connection,
    task_id: &str,
    workspace_id: &str,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE tasks SET workspace_id = ?2 WHERE id = ?1",
        params![task_id, workspace_id],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "task",
            id: task_id.to_string(),
        });
    }
    Ok(())
}

/// Binds a run to a workspace (sets `workspace_id` on an existing run row).
pub fn bind_run_workspace(
    conn: &Connection,
    run_id: &str,
    workspace_id: &str,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE runs SET workspace_id = ?2 WHERE id = ?1",
        params![run_id, workspace_id],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "run",
            id: run_id.to_string(),
        });
    }
    Ok(())
}

/// Returns the currently focused workspace id, if any. Delegates to
/// `workspace_open_state::find_focused`.
pub fn find_focused_workspace_id(conn: &Connection) -> Result<Option<String>, StoreError> {
    Ok(super::workspace_open_state::find_focused(conn)?.map(|row| row.workspace_id))
}

/// Isolation violation record: a run whose `workspace_id` differs from its
/// parent task's `workspace_id`, indicating cross-workspace data leakage.
#[derive(Debug, Clone)]
pub struct IsolationViolation {
    pub run_id: String,
    pub task_id: String,
    pub run_workspace_id: Option<String>,
    pub task_workspace_id: Option<String>,
}

/// Detects workspace isolation violations: runs whose `workspace_id` differs
/// from their parent task's `workspace_id`. Returns an empty vec when all
/// runs are properly isolated. Used for audit logging and alerting (task 10.2.3).
pub fn detect_isolation_violations(conn: &Connection) -> Result<Vec<IsolationViolation>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.task_id, r.workspace_id, t.workspace_id
         FROM runs r
         JOIN tasks t ON r.task_id = t.id
         WHERE r.workspace_id IS NOT t.workspace_id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(IsolationViolation {
            run_id: row.get(0)?,
            task_id: row.get(1)?,
            run_workspace_id: row.get(2)?,
            task_workspace_id: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

// ---------------------------------------------------------------- helpers

fn schedule_select(where_clause: &str) -> String {
    format!(
        "SELECT id, name, cron_expr, task_title, task_description, enabled,
         last_triggered_at, next_trigger_at, created_at, updated_at,
         target_kind, agent_kind, agent_ref_id, team_id, session_mode, session_id, auto_dispatch
         FROM schedules {where_clause}"
    )
}

/// Distinguishes "row gone" from "status moved on" after a guarded UPDATE.
fn guard_rows(n: usize, entity: &'static str, id: &str, expected: &str) -> Result<(), StoreError> {
    if n > 0 {
        return Ok(());
    }
    Err(StoreError::Conflict {
        entity,
        id: id.to_string(),
        expected: expected.to_string(),
    })
}

fn parse_status<T: Copy>(
    col: usize,
    raw: &str,
    parse: fn(&str) -> Option<T>,
) -> rusqlite::Result<T> {
    parse(raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            col,
            rusqlite::types::Type::Text,
            format!("unknown status value: {raw}").into(),
        )
    })
}

fn row_to_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let status: String = row.get(4)?;
    Ok(Task {
        id: row.get(0)?,
        session_id: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        status: parse_status(4, &status, TaskStatus::parse)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn row_to_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<Run> {
    let status: String = row.get(3)?;
    Ok(Run {
        id: row.get(0)?,
        task_id: row.get(1)?,
        session_id: row.get(2)?,
        status: parse_status(3, &status, RunState::parse)?,
        heartbeat_at: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn row_to_approval(row: &rusqlite::Row<'_>) -> rusqlite::Result<Approval> {
    let decision: String = row.get(4)?;
    Ok(Approval {
        id: row.get(0)?,
        run_id: row.get(1)?,
        tool_name: row.get(2)?,
        arguments_json: row.get(3)?,
        decision: parse_status(4, &decision, ApprovalDecision::parse)?,
        decided_at: row.get(5)?,
        created_at: row.get(6)?,
    })
}

fn row_to_schedule(row: &rusqlite::Row<'_>) -> rusqlite::Result<Schedule> {
    let target_kind_str: String = row.get(10)?;
    let target_kind =
        ScheduleTargetKind::parse(&target_kind_str).unwrap_or(ScheduleTargetKind::Task);
    let agent_kind: Option<String> = row.get(11)?;
    let agent_ref_id: Option<String> = row.get(12)?;
    let agent = match (agent_kind.as_deref(), agent_ref_id) {
        (Some(k), Some(id)) => AgentRefKind::parse(k).map(|kind| (kind, id)),
        _ => None,
    };
    let session_mode_str: String = row.get(14)?;
    let session_mode =
        ScheduleSessionMode::parse(&session_mode_str).unwrap_or(ScheduleSessionMode::PerTrigger);
    Ok(Schedule {
        id: row.get(0)?,
        name: row.get(1)?,
        cron_expr: row.get(2)?,
        task_title: row.get(3)?,
        task_description: row.get(4)?,
        enabled: row.get::<_, i64>(5)? != 0,
        last_triggered_at: row.get(6)?,
        next_trigger_at: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        target_kind,
        agent,
        team_id: row.get(13)?,
        session_mode,
        session_id: row.get(15)?,
        auto_dispatch: row.get::<_, i64>(16)? != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    fn task(id: &str, status: TaskStatus) -> Task {
        Task {
            id: id.into(),
            session_id: None,
            title: format!("task {id}"),
            description: String::new(),
            status,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn run(id: &str, task_id: &str, status: RunState) -> Run {
        Run {
            id: id.into(),
            task_id: task_id.into(),
            session_id: "s1".into(),
            status,
            heartbeat_at: 1,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn approval(id: &str, run_id: &str) -> Approval {
        Approval {
            id: id.into(),
            run_id: run_id.into(),
            tool_name: "fs.write".into(),
            arguments_json: r#"{"path":"a.txt"}"#.into(),
            decision: ApprovalDecision::Pending,
            decided_at: None,
            created_at: 1,
        }
    }

    fn schedule(id: &str, name: &str, enabled: bool) -> Schedule {
        Schedule {
            id: id.into(),
            name: name.into(),
            cron_expr: "@every 60".into(),
            task_title: "tick".into(),
            task_description: String::new(),
            enabled,
            last_triggered_at: None,
            next_trigger_at: Some(100),
            created_at: 1,
            updated_at: 1,
            target_kind: ScheduleTargetKind::Task,
            agent: None,
            team_id: None,
            session_mode: ScheduleSessionMode::PerTrigger,
            session_id: None,
            auto_dispatch: true,
        }
    }

    // ---- tasks

    #[test]
    fn task_insert_get_list_roundtrip() {
        let conn = db();
        insert_task(&conn, &task("t1", TaskStatus::Queued)).unwrap();
        insert_task(&conn, &task("t2", TaskStatus::Done)).unwrap();
        assert_eq!(get_task(&conn, "t1").unwrap().title, "task t1");
        assert_eq!(list_tasks(&conn, 10).unwrap().len(), 2);
        assert_eq!(
            list_tasks_by_status(&conn, TaskStatus::Done, 10).unwrap()[0].id,
            "t2"
        );
    }

    #[test]
    fn task_status_guard_happy_and_conflict_and_missing() {
        let conn = db();
        insert_task(&conn, &task("t1", TaskStatus::Queued)).unwrap();

        // happy path
        update_task_status(&conn, "t1", TaskStatus::Queued, TaskStatus::Running, 2).unwrap();
        assert_eq!(get_task(&conn, "t1").unwrap().status, TaskStatus::Running);
        assert_eq!(get_task(&conn, "t1").unwrap().updated_at, 2);

        // stale expectation → Conflict
        assert!(matches!(
            update_task_status(&conn, "t1", TaskStatus::Queued, TaskStatus::Done, 3),
            Err(StoreError::Conflict { entity: "task", .. })
        ));

        // unknown id → Conflict carries the guard info; get reports NotFound
        assert!(matches!(
            update_task_status(&conn, "ghost", TaskStatus::Queued, TaskStatus::Done, 3),
            Err(StoreError::Conflict { entity: "task", .. })
        ));
        assert!(matches!(
            get_task(&conn, "ghost"),
            Err(StoreError::NotFound { entity: "task", .. })
        ));
    }

    #[test]
    fn task_duplicate_insert_fails() {
        let conn = db();
        insert_task(&conn, &task("dup", TaskStatus::Backlog)).unwrap();
        assert!(insert_task(&conn, &task("dup", TaskStatus::Backlog)).is_err());
    }

    #[test]
    fn task_rejects_invalid_status_via_check_constraint() {
        let conn = db();
        assert!(conn
            .execute(
                "INSERT INTO tasks (id, title, status, created_at, updated_at)
                 VALUES ('bad', 'x', 'exploded', 1, 1)",
                [],
            )
            .is_err());
    }

    #[test]
    fn task_delete_true_then_false_roundtrip() {
        let conn = db();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        insert_task(&conn, &task("t1", TaskStatus::Queued)).unwrap();

        assert!(delete(&conn, "t1").unwrap());
        assert!(matches!(
            get_task(&conn, "t1"),
            Err(StoreError::NotFound { entity: "task", .. })
        ));
        assert!(!delete(&conn, "t1").unwrap());
        assert!(list_tasks(&conn, 10).unwrap().is_empty());
    }

    #[test]
    fn task_delete_cascade_removes_children_and_reports_missing() {
        let conn = db();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        insert_task(&conn, &task("t1", TaskStatus::Done)).unwrap();
        insert_task(&conn, &task("t2", TaskStatus::Queued)).unwrap();
        insert_run(&conn, &run("r1", "t1", RunState::Succeeded)).unwrap();
        insert_run(&conn, &run("r2", "t1", RunState::Failed)).unwrap();
        insert_run(&conn, &run("r3", "t2", RunState::Queued)).unwrap();
        insert_approval(&conn, &approval("a1", "r1")).unwrap();

        // FK enforcement: the bare parent delete cannot leapfrog children.
        assert!(delete(&conn, "t1").is_err());

        assert!(delete_cascade(&conn, "t1").unwrap());
        assert!(matches!(
            get_task(&conn, "t1"),
            Err(StoreError::NotFound { entity: "task", .. })
        ));
        assert!(list_runs_by_task(&conn, "t1").unwrap().is_empty());
        assert!(matches!(
            get_approval(&conn, "a1"),
            Err(StoreError::NotFound {
                entity: "approval",
                ..
            })
        ));

        // Sibling rows survive the cascade untouched.
        assert_eq!(get_task(&conn, "t2").unwrap().id, "t2");
        assert_eq!(list_runs_by_task(&conn, "t2").unwrap()[0].id, "r3");

        // Second pass: nothing left → false, nothing written.
        assert!(!delete_cascade(&conn, "t1").unwrap());

        // Standalone child helpers.
        assert_eq!(delete_approvals_for_task(&conn, "t2").unwrap(), 0);
        assert_eq!(delete_runs_for_task(&conn, "t2").unwrap(), 1);
        assert!(list_runs_by_task(&conn, "t2").unwrap().is_empty());
    }

    // ---- runs

    #[test]
    fn run_insert_get_lists_and_heartbeat() {
        let conn = db();
        insert_task(&conn, &task("t1", TaskStatus::Running)).unwrap();
        insert_run(&conn, &run("r1", "t1", RunState::Queued)).unwrap();
        insert_run(&conn, &run("r2", "t1", RunState::Running)).unwrap();

        assert_eq!(get_run(&conn, "r1").unwrap().status, RunState::Queued);
        assert_eq!(list_runs_by_task(&conn, "t1").unwrap().len(), 2);
        assert_eq!(
            list_runs_by_status(&conn, RunState::Running).unwrap()[0].id,
            "r2"
        );

        heartbeat(&conn, "r1", 42).unwrap();
        assert_eq!(get_run(&conn, "r1").unwrap().heartbeat_at, 42);
        assert!(matches!(
            heartbeat(&conn, "ghost", 1),
            Err(StoreError::NotFound { entity: "run", .. })
        ));
    }

    #[test]
    fn run_status_guard_full_lifecycle_then_stale_conflict() {
        let conn = db();
        insert_task(&conn, &task("t1", TaskStatus::Running)).unwrap();
        insert_run(&conn, &run("r1", "t1", RunState::Queued)).unwrap();

        // legal lifecycle driven through the repo guard
        for (from, to) in [
            (RunState::Queued, RunState::Running),
            (RunState::Running, RunState::AwaitingApproval),
            (RunState::AwaitingApproval, RunState::Running),
            (RunState::Running, RunState::Interrupted),
            (RunState::Interrupted, RunState::Queued),
            (RunState::Queued, RunState::Running),
            (RunState::Running, RunState::Succeeded),
        ] {
            update_run_status(&conn, "r1", from, to, 9).unwrap();
        }
        assert_eq!(get_run(&conn, "r1").unwrap().status, RunState::Succeeded);

        // terminal state: any further guarded move conflicts
        assert!(matches!(
            update_run_status(&conn, "r1", RunState::Running, RunState::Failed, 10),
            Err(StoreError::Conflict { entity: "run", .. })
        ));
    }

    #[test]
    fn run_requires_existing_task_fk() {
        let conn = db();
        assert!(insert_run(&conn, &run("r1", "missing-task", RunState::Queued)).is_err());
    }

    // ---- approvals

    #[test]
    fn approval_pending_flow_approve_once_only() {
        let conn = db();
        insert_task(&conn, &task("t1", TaskStatus::Running)).unwrap();
        insert_run(&conn, &run("r1", "t1", RunState::AwaitingApproval)).unwrap();
        insert_approval(&conn, &approval("a1", "r1")).unwrap();

        assert_eq!(list_pending_approvals(&conn).unwrap().len(), 1);
        assert_eq!(list_approvals_by_run(&conn, "r1").unwrap().len(), 1);

        resolve_approval(&conn, "a1", ApprovalDecision::Approved, 7).unwrap();
        let got = get_approval(&conn, "a1").unwrap();
        assert_eq!(got.decision, ApprovalDecision::Approved);
        assert_eq!(got.decided_at, Some(7));
        assert!(list_pending_approvals(&conn).unwrap().is_empty());

        // decision is final — second resolve hits the pending guard
        assert!(matches!(
            resolve_approval(&conn, "a1", ApprovalDecision::Denied, 8),
            Err(StoreError::Conflict {
                entity: "approval",
                ..
            })
        ));
    }

    #[test]
    fn approval_deny_and_unknown_id() {
        let conn = db();
        insert_task(&conn, &task("t1", TaskStatus::Running)).unwrap();
        insert_run(&conn, &run("r1", "t1", RunState::AwaitingApproval)).unwrap();
        insert_approval(&conn, &approval("a1", "r1")).unwrap();
        resolve_approval(&conn, "a1", ApprovalDecision::Denied, 3).unwrap();
        assert_eq!(
            get_approval(&conn, "a1").unwrap().decision,
            ApprovalDecision::Denied
        );
        assert!(matches!(
            resolve_approval(&conn, "ghost", ApprovalDecision::Approved, 4),
            Err(StoreError::Conflict {
                entity: "approval",
                ..
            })
        ));
        assert!(matches!(
            get_approval(&conn, "ghost"),
            Err(StoreError::NotFound {
                entity: "approval",
                ..
            })
        ));
    }

    // ---- schedules

    #[test]
    fn schedule_crud_and_due_scan() {
        let conn = db();
        insert_schedule(&conn, &schedule("s1", "nightly", true)).unwrap();
        insert_schedule(&conn, &schedule("s2", "paused", false)).unwrap();

        assert_eq!(get_schedule_by_name(&conn, "nightly").unwrap().id, "s1");
        assert!(matches!(
            get_schedule_by_name(&conn, "nope"),
            Err(StoreError::NotFound {
                entity: "schedule",
                ..
            })
        ));

        // due scan only picks enabled + overdue rows
        let due = due_schedules(&conn, 150).unwrap();
        assert_eq!(
            due.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["s1"]
        );
        assert!(due_schedules(&conn, 99).unwrap().is_empty());

        mark_schedule_triggered(&conn, "s1", 150, Some(210)).unwrap();
        let fired = get_schedule(&conn, "s1").unwrap();
        assert_eq!(fired.last_triggered_at, Some(150));
        assert_eq!(fired.next_trigger_at, Some(210));
        assert!(due_schedules(&conn, 150).unwrap().is_empty());

        set_schedule_enabled(&conn, "s1", false, 160).unwrap();
        assert!(!get_schedule(&conn, "s1").unwrap().enabled);
        assert!(matches!(
            set_schedule_enabled(&conn, "ghost", true, 1),
            Err(StoreError::NotFound {
                entity: "schedule",
                ..
            })
        ));
        assert_eq!(list_schedules(&conn, 10).unwrap().len(), 2);
    }

    #[test]
    fn schedule_name_is_unique() {
        let conn = db();
        insert_schedule(&conn, &schedule("s1", "same", true)).unwrap();
        assert!(insert_schedule(&conn, &schedule("s2", "same", true)).is_err());
    }

    #[test]
    fn schedule_update_edits_fields_and_reports_missing() {
        let conn = db();
        insert_schedule(&conn, &schedule("s1", "old", true)).unwrap();
        let mut s = get_schedule(&conn, "s1").unwrap();
        s.cron_expr = "0 9 * * *".into();
        s.task_title = "morning".into();
        s.updated_at = 5;
        update_schedule(&conn, &s).unwrap();
        let got = get_schedule(&conn, "s1").unwrap();
        assert_eq!(got.cron_expr, "0 9 * * *");
        assert_eq!(got.task_title, "morning");

        s.id = "ghost".into();
        assert!(matches!(
            update_schedule(&conn, &s),
            Err(StoreError::NotFound {
                entity: "schedule",
                ..
            })
        ));
    }

    #[test]
    fn detect_isolation_violations_finds_mismatched_workspace() {
        let conn = db();
        let t1 = task("t1", TaskStatus::Queued);
        insert_task(&conn, &t1).unwrap();
        bind_task_workspace(&conn, "t1", "ws-a").unwrap();

        let r1 = run("r1", "t1", RunState::Queued);
        insert_run(&conn, &r1).unwrap();
        bind_run_workspace(&conn, "r1", "ws-b").unwrap();

        let violations = detect_isolation_violations(&conn).unwrap();
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].run_id, "r1");
        assert_eq!(violations[0].task_id, "t1");
        assert_eq!(violations[0].run_workspace_id.as_deref(), Some("ws-b"));
        assert_eq!(violations[0].task_workspace_id.as_deref(), Some("ws-a"));
    }

    #[test]
    fn detect_isolation_violations_none_when_consistent() {
        let conn = db();
        let t1 = task("t1", TaskStatus::Queued);
        insert_task(&conn, &t1).unwrap();
        bind_task_workspace(&conn, "t1", "ws-a").unwrap();

        let r1 = run("r1", "t1", RunState::Queued);
        insert_run(&conn, &r1).unwrap();
        bind_run_workspace(&conn, "r1", "ws-a").unwrap();

        let violations = detect_isolation_violations(&conn).unwrap();
        assert!(violations.is_empty());
    }
}
