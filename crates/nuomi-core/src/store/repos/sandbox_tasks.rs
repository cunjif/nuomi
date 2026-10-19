//! Sandbox-task repository (`sandbox_tasks` table, migration 0037).
//!
//! Row-level persistence for the sandbox task lifecycle (ADR-0022):
//! inserts start at `queued`; `mark_running` / `mark_terminal` apply state
//! transitions; `append_output` grows the inline stdout/stderr columns (the
//! manager treats `stdout.len()` as its fetch cursor); `set_artifact`
//! records the artifact location or the retryable flag. Every state
//! transition is audited as a `sandbox.task_state_changed` event by the
//! task manager BEFORE the row update — this repo never appends events
//! itself (single-writer discipline for the audit trail).

use rusqlite::{params, Connection, OptionalExtension};

use crate::sandbox::types::{
    dirs_from_json, SandboxLimits, SandboxResourceUsage, SandboxTaskStatus, SandboxTaskType,
};
use crate::store::StoreError;

/// A `sandbox_tasks` row (mirrors migrations 0037 + 0038).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxTaskRecord {
    pub id: String,
    pub task_type: SandboxTaskType,
    pub runtime: String,
    pub status: SandboxTaskStatus,
    pub source: String,
    /// Stored as the `authorized_dirs` JSON array column.
    pub authorized_dirs: Vec<String>,
    /// Stored as the `limits` JSON object column.
    pub limits: SandboxLimits,
    pub stdout: String,
    pub stderr: String,
    pub artifact_ref: Option<String>,
    pub artifact_retryable: bool,
    pub exit_info: Option<String>,
    pub duration_ms: Option<u64>,
    /// Executor-reported terminal usage, stored as the `usage` JSON object
    /// column (migration 0038); `None` when the executor never reported.
    pub usage: Option<SandboxResourceUsage>,
    /// unix-ms
    pub created_at: i64,
    /// unix-ms; `None` while non-terminal
    pub finished_at: Option<i64>,
}

const ENTITY: &str = "sandbox_task";

/// Inserts a task row (the manager always starts at `queued`).
pub fn insert(conn: &Connection, record: &SandboxTaskRecord) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO sandbox_tasks
           (id, task_type, runtime, status, source, authorized_dirs, limits,
            stdout, stderr, artifact_ref, artifact_retryable, exit_info,
            duration_ms, usage, created_at, finished_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            record.id,
            record.task_type.as_str(),
            record.runtime,
            record.status.as_str(),
            record.source,
            serde_json::to_string(&record.authorized_dirs)?,
            serde_json::to_string(&record.limits)?,
            record.stdout,
            record.stderr,
            record.artifact_ref,
            record.artifact_retryable,
            record.exit_info,
            record.duration_ms,
            record
                .usage
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            record.created_at,
            record.finished_at,
        ],
    )?;
    Ok(())
}

/// Finds a task by id.
pub fn find_by_id(conn: &Connection, id: &str) -> Result<Option<SandboxTaskRecord>, StoreError> {
    conn.query_row(
        "SELECT id, task_type, runtime, status, source, authorized_dirs, limits,
                stdout, stderr, artifact_ref, artifact_retryable, exit_info,
                duration_ms, usage, created_at, finished_at
         FROM sandbox_tasks WHERE id = ?1",
        params![id],
        row_to_record,
    )
    .optional()
    .map_err(StoreError::Sqlite)
}

/// Lists recent tasks, newest first (index idx_sandbox_tasks_created).
pub fn list_recent(conn: &Connection, limit: u32) -> Result<Vec<SandboxTaskRecord>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, task_type, runtime, status, source, authorized_dirs, limits,
                stdout, stderr, artifact_ref, artifact_retryable, exit_info,
                duration_ms, usage, created_at, finished_at
         FROM sandbox_tasks ORDER BY created_at DESC, id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], row_to_record)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Applies the queued → running transition.
pub fn mark_running(conn: &Connection, id: &str) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE sandbox_tasks SET status = 'running' WHERE id = ?1 AND status = 'queued'",
        params![id],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: ENTITY,
            id: id.to_string(),
        });
    }
    Ok(())
}

/// Applies a terminal transition (succeeded / failed / timed_out /
/// cancelled), stamping exit info, duration and finished_at.
pub fn mark_terminal(
    conn: &Connection,
    id: &str,
    status: SandboxTaskStatus,
    exit_info: Option<&str>,
    duration_ms: Option<u64>,
    finished_at: i64,
) -> Result<(), StoreError> {
    debug_assert!(status.is_terminal());
    let n = conn.execute(
        "UPDATE sandbox_tasks
         SET status = ?2, exit_info = ?3, duration_ms = ?4, finished_at = ?5
         WHERE id = ?1 AND status NOT IN
           ('succeeded', 'failed', 'timed_out', 'cancelled')",
        params![id, status.as_str(), exit_info, duration_ms, finished_at],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: ENTITY,
            id: id.to_string(),
        });
    }
    Ok(())
}

/// Records the executor-reported terminal usage onto the row
/// (migration 0038). Called inside the audited terminal transition's
/// transaction, after `mark_terminal`.
pub fn set_usage(
    conn: &Connection,
    id: &str,
    usage: &SandboxResourceUsage,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE sandbox_tasks SET usage = ?2 WHERE id = ?1",
        params![id, serde_json::to_string(usage)?],
    )?;
    Ok(())
}

/// Grows the inline output columns (the executor-side delta since the
/// row's current cursor, i.e. `stdout.len()` / `stderr.len()`).
pub fn append_output(
    conn: &Connection,
    id: &str,
    stdout_delta: &str,
    stderr_delta: &str,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE sandbox_tasks SET stdout = stdout || ?2, stderr = stderr || ?3 WHERE id = ?1",
        params![id, stdout_delta, stderr_delta],
    )?;
    Ok(())
}

/// Records the artifact location or the retryable flag (artifact return
/// failure keeps the task `succeeded` with `artifact_retryable = 1`).
pub fn set_artifact(
    conn: &Connection,
    id: &str,
    artifact_ref: Option<&str>,
    retryable: bool,
) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE sandbox_tasks SET artifact_ref = ?2, artifact_retryable = ?3 WHERE id = ?1",
        params![id, artifact_ref, retryable],
    )?;
    Ok(())
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<SandboxTaskRecord> {
    let task_type_raw: String = row.get(1)?;
    let status_raw: String = row.get(3)?;
    let dirs_raw: String = row.get(5)?;
    let limits_raw: String = row.get(6)?;
    let retryable: i64 = row.get(10)?;
    let usage_raw: Option<String> = row.get(13)?;
    Ok(SandboxTaskRecord {
        id: row.get(0)?,
        task_type: parse_enum(1, &task_type_raw, SandboxTaskType::parse)?,
        runtime: row.get(2)?,
        status: parse_enum(3, &status_raw, SandboxTaskStatus::parse)?,
        source: row.get(4)?,
        authorized_dirs: dirs_from_json(&dirs_raw).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
        })?,
        limits: SandboxLimits::from_json(&limits_raw).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e))
        })?,
        stdout: row.get(7)?,
        stderr: row.get(8)?,
        artifact_ref: row.get(9)?,
        artifact_retryable: retryable != 0,
        exit_info: row.get(11)?,
        duration_ms: row.get(12)?,
        usage: usage_raw
            .map(|raw| serde_json::from_str(&raw))
            .transpose()
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    13,
                    rusqlite::types::Type::Text,
                    format!("corrupt sandbox usage JSON: {e}").into(),
                )
            })?,
        created_at: row.get(14)?,
        finished_at: row.get(15)?,
    })
}

/// Maps a CHECK-constrained TEXT column onto its enum; anything else is a
/// corrupt row surfaced as a conversion failure (mirrors `store::json_col`).
fn parse_enum<T>(col: usize, raw: &str, parse: fn(&str) -> Option<T>) -> rusqlite::Result<T> {
    parse(raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            col,
            rusqlite::types::Type::Text,
            format!("unknown sandbox enum value: {raw}").into(),
        )
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

    fn queued_record(id: &str, created_at: i64) -> SandboxTaskRecord {
        SandboxTaskRecord {
            id: id.to_string(),
            task_type: SandboxTaskType::BuildApp,
            runtime: "python".to_string(),
            status: SandboxTaskStatus::Queued,
            source: "cargo build --release".to_string(),
            authorized_dirs: vec!["C:\\proj\\nuomi".to_string()],
            limits: SandboxLimits {
                timeout_secs: 300,
                cpu_percent: Some(50),
                memory_mb: Some(2048),
                network_enabled: true,
            },
            stdout: String::new(),
            stderr: String::new(),
            artifact_ref: None,
            artifact_retryable: false,
            exit_info: None,
            duration_ms: None,
            usage: None,
            created_at,
            finished_at: None,
        }
    }

    #[test]
    fn insert_find_roundtrips_every_column() {
        let conn = db();
        let record = queued_record("t1", 42);
        insert(&conn, &record).unwrap();
        let back = find_by_id(&conn, "t1").unwrap().unwrap();
        assert_eq!(back, record);
        assert_eq!(
            back.limits,
            SandboxLimits {
                timeout_secs: 300,
                cpu_percent: Some(50),
                memory_mb: Some(2048),
                network_enabled: true,
            }
        );
        assert_eq!(back.authorized_dirs, vec!["C:\\proj\\nuomi".to_string()]);
    }

    #[test]
    fn find_missing_returns_none() {
        let conn = db();
        assert!(find_by_id(&conn, "ghost").unwrap().is_none());
    }

    #[test]
    fn usage_roundtrips_via_insert_and_set_usage() {
        let conn = db();
        let mut with_usage = queued_record("t-usage", 7);
        with_usage.usage = Some(SandboxResourceUsage {
            cpu_time_ms: Some(120),
            peak_memory_mb: Some(64),
        });
        insert(&conn, &with_usage).unwrap();
        assert_eq!(
            find_by_id(&conn, "t-usage").unwrap().unwrap().usage,
            with_usage.usage
        );

        let plain = queued_record("t-plain", 8);
        insert(&conn, &plain).unwrap();
        assert_eq!(find_by_id(&conn, "t-plain").unwrap().unwrap().usage, None);
        set_usage(
            &conn,
            "t-plain",
            &SandboxResourceUsage {
                cpu_time_ms: None,
                peak_memory_mb: Some(32),
            },
        )
        .unwrap();
        assert_eq!(
            find_by_id(&conn, "t-plain").unwrap().unwrap().usage,
            Some(SandboxResourceUsage {
                cpu_time_ms: None,
                peak_memory_mb: Some(32),
            })
        );
        // list_recent reads the same column.
        assert_eq!(
            list_recent(&conn, 10).unwrap()[0].usage,
            Some(SandboxResourceUsage {
                cpu_time_ms: None,
                peak_memory_mb: Some(32),
            })
        );
    }

    #[test]
    fn list_recent_orders_newest_first_with_limit() {
        let conn = db();
        for (i, created_at) in [10_i64, 30, 20].into_iter().enumerate() {
            insert(&conn, &queued_record(&format!("t{i}"), created_at)).unwrap();
        }
        let all = list_recent(&conn, 10).unwrap();
        assert_eq!(
            all.iter().map(|r| r.created_at).collect::<Vec<_>>(),
            vec![30, 20, 10]
        );
        let top2 = list_recent(&conn, 2).unwrap();
        assert_eq!(top2.len(), 2);
        assert_eq!(top2[0].created_at, 30);
    }

    #[test]
    fn lifecycle_updates_stamp_state_and_finished_at() {
        let conn = db();
        insert(&conn, &queued_record("t1", 1)).unwrap();

        mark_running(&conn, "t1").unwrap();
        assert_eq!(
            find_by_id(&conn, "t1").unwrap().unwrap().status,
            SandboxTaskStatus::Running
        );

        mark_terminal(
            &conn,
            "t1",
            SandboxTaskStatus::Succeeded,
            Some("exit 0"),
            Some(1_234),
            99,
        )
        .unwrap();
        let done = find_by_id(&conn, "t1").unwrap().unwrap();
        assert_eq!(done.status, SandboxTaskStatus::Succeeded);
        assert_eq!(done.exit_info.as_deref(), Some("exit 0"));
        assert_eq!(done.duration_ms, Some(1_234));
        assert_eq!(done.finished_at, Some(99));

        // 二次终态迁移被拒绝（状态机唯一终态）
        assert!(matches!(
            mark_terminal(&conn, "t1", SandboxTaskStatus::Failed, None, None, 100),
            Err(StoreError::NotFound { .. })
        ));
        // 已终态任务不能再 mark_running
        assert!(matches!(
            mark_running(&conn, "t1"),
            Err(StoreError::NotFound { .. })
        ));
    }

    #[test]
    fn mark_running_on_missing_task_is_not_found() {
        let conn = db();
        assert!(matches!(
            mark_running(&conn, "ghost"),
            Err(StoreError::NotFound {
                entity: "sandbox_task",
                ..
            })
        ));
    }

    #[test]
    fn append_output_concatenates_deltas() {
        let conn = db();
        insert(&conn, &queued_record("t1", 1)).unwrap();
        append_output(&conn, "t1", "hello\n", "").unwrap();
        append_output(&conn, "t1", "world\n", "warn").unwrap();
        let row = find_by_id(&conn, "t1").unwrap().unwrap();
        assert_eq!(row.stdout, "hello\nworld\n");
        assert_eq!(row.stderr, "warn");
    }

    #[test]
    fn set_artifact_updates_ref_and_retryable() {
        let conn = db();
        insert(&conn, &queued_record("t1", 1)).unwrap();
        set_artifact(
            &conn,
            "t1",
            Some("C:\\nuomi\\.nuomi\\sandbox\\t1\\app.exe"),
            false,
        )
        .unwrap();
        assert_eq!(
            find_by_id(&conn, "t1")
                .unwrap()
                .unwrap()
                .artifact_ref
                .as_deref(),
            Some("C:\\nuomi\\.nuomi\\sandbox\\t1\\app.exe")
        );
        set_artifact(&conn, "t1", None, true).unwrap();
        let row = find_by_id(&conn, "t1").unwrap().unwrap();
        assert!(row.artifact_retryable);
        assert!(row.artifact_ref.is_none());
    }

    #[test]
    fn spec_fields_survive_json_columns_for_both_task_types() {
        let conn = db();
        for task_type in [SandboxTaskType::RunCode, SandboxTaskType::BuildApp] {
            let id = format!("t-{task_type}");
            let mut record = queued_record(&id, 1);
            record.task_type = task_type;
            record.authorized_dirs = vec![];
            insert(&conn, &record).unwrap();
            assert_eq!(find_by_id(&conn, &id).unwrap().unwrap(), record);
        }
    }

    #[test]
    fn check_constraint_rejects_illegal_enum_values() {
        let conn = db();
        let record = queued_record("t1", 1);
        insert(&conn, &record).unwrap();
        assert!(conn
            .execute(
                "UPDATE sandbox_tasks SET status = 'paused' WHERE id = 't1'",
                []
            )
            .is_err());
        assert!(conn
            .execute(
                "UPDATE sandbox_tasks SET task_type = 'deploy' WHERE id = 't1'",
                []
            )
            .is_err());
    }
}
