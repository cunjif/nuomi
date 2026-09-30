//! Append-only migrations runner.
//!
//! Each migration is an embedded SQL file from the workspace `migrations/`
//! directory. Files are executed lexicographically; applied versions are
//! tracked via `PRAGMA user_version`. Shipped files are never edited.

use rusqlite::Connection;

use super::StoreError;

const MIGRATIONS: &[(i64, &str, &str)] = &[
    (
        1,
        "0001_init",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0001_init.sql"
        )),
    ),
    (
        2,
        "0002_tasks_runs",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0002_tasks_runs.sql"
        )),
    ),
    (
        3,
        "0003_agent_profiles",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0003_agent_profiles.sql"
        )),
    ),
    (
        4,
        "0004_integrations",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0004_integrations.sql"
        )),
    ),
    (
        5,
        "0005_memory_fts",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0005_memory_fts.sql"
        )),
    ),
    (
        6,
        "0006_change_log",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0006_change_log.sql"
        )),
    ),
    (
        7,
        "0007_session_cache_scope",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0007_session_cache_scope.sql"
        )),
    ),
    (
        8,
        "0008_app_settings",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0008_app_settings.sql"
        )),
    ),
    (
        10,
        "0010_role_capabilities",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0010_role_capabilities.sql"
        )),
    ),
    (
        11,
        "0011_conversation_kind",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0011_conversation_kind.sql"
        )),
    ),
    (
        12,
        "0012_attachments",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0012_attachments.sql"
        )),
    ),
    (
        13,
        "0013_schedule_conversations",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0013_schedule_conversations.sql"
        )),
    ),
    (
        14,
        "0014_conversation_meta",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0014_conversation_meta.sql"
        )),
    ),
    (
        15,
        "0015_context_injections",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0015_context_injections.sql"
        )),
    ),
    (
        16,
        "0016_workspaces",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0016_workspaces.sql"
        )),
    ),
    (
        17,
        "0017_sessions_workspace",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0017_sessions_workspace.sql"
        )),
    ),
    (
        18,
        "0018_task_failed_status",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0018_task_failed_status.sql"
        )),
    ),
    (
        19,
        "0019_agent_profile_model_id",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0019_agent_profile_model_id.sql"
        )),
    ),
    (
        20,
        "0020_conversation_role_agent_cli_session",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0020_conversation_role_agent_cli_session.sql"
        )),
    ),
    (
        21,
        "0021_remove_primary_agent",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0021_remove_primary_agent.sql"
        )),
    ),
    (
        22,
        "0022_session_soft_delete",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0022_session_soft_delete.sql"
        )),
    ),
    (
        23,
        "0023_message_queue",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0023_message_queue.sql"
        )),
    ),
    (
        24,
        "0024_workspace_open_state",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0024_workspace_open_state.sql"
        )),
    ),
    (
        25,
        "0025_tasks_runs_workspace",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0025_tasks_runs_workspace.sql"
        )),
    ),
    (
        26,
        "0026_schedules_workspace",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0026_schedules_workspace.sql"
        )),
    ),
];

/// Applies all pending migrations inside transactions, updating `user_version`.
pub fn run(conn: &Connection) -> Result<(), StoreError> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for &(version, name, sql) in MIGRATIONS {
        if version <= current {
            continue;
        }
        conn.execute_batch("BEGIN IMMEDIATE;")
            .map_err(|source| StoreError::Migration {
                name: name.to_string(),
                source,
            })?;
        let result = conn
            .execute_batch(sql)
            .and_then(|_| conn.pragma_update(None, "user_version", version))
            .and_then(|_| conn.execute_batch("COMMIT;"));
        match result {
            Ok(()) => tracing::info!(version, name, "migration applied"),
            Err(source) => {
                let _ = conn.execute_batch("ROLLBACK;");
                return Err(StoreError::Migration {
                    name: name.to_string(),
                    source,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_all_migrations_and_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, MIGRATIONS.last().unwrap().0);
        // second run is a no-op
        run(&conn).unwrap();
        let v2: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, v2);
    }

    #[test]
    fn core_tables_exist_after_init() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        for table in [
            "sessions",
            "events",
            "provider_configs",
            "roles",
            "teams",
            "memory_entries",
            "whiteboard_notes",
            "prompt_versions",
            "artifacts",
            "tasks",
            "runs",
            "approvals",
            "schedules",
            "agent_profiles",
            "integrations",
            "attachments",
            "workspaces",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "table {table} missing");
        }
    }

    #[test]
    fn events_table_rejects_update_and_delete() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES ('s1','','1','1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events (aggregate_type, aggregate_id, kind, payload, seq, created_at)
             VALUES ('session','s1','message','{}',1,1)",
            [],
        )
        .unwrap();
        assert!(conn.execute("UPDATE events SET kind='x'", []).is_err());
        assert!(conn.execute("DELETE FROM events", []).is_err());
    }

    #[test]
    fn workspaces_root_path_unique_and_placeholder_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        // root_path UNIQUE 约束：插入重复路径必须失败。
        conn.execute(
            "INSERT INTO workspaces (id, root_path, color_tag, created_at, is_active)
             VALUES ('w1', 'C:\\ws', 'paper-yellow', 1, 1)",
            [],
        )
        .unwrap();
        let dup = conn.execute(
            "INSERT INTO workspaces (id, root_path, color_tag, created_at, is_active)
             VALUES ('w2', 'C:\\ws', 'paper-yellow', 2, 0)",
            [],
        );
        assert!(dup.is_err(), "root_path UNIQUE must reject duplicates");

        // 占位 INSERT 幂等：在已有 app_settings.workspace_root 且 workspaces 非空时，
        // 重新执行 0016 SQL 的 INSERT 子句不应产生新行（NOT EXISTS 守卫）。
        conn.execute(
            "INSERT INTO app_settings (key, value, updated_at)
             VALUES ('workspace_root', 'C:\\ws', 1)",
            [],
        )
        .unwrap();
        let before: i64 = conn
            .query_row("SELECT count(*) FROM workspaces", [], |r| r.get(0))
            .unwrap();
        // 模拟迁移 runner 重跑 0016 的 INSERT 语句（NOT EXISTS 守卫生效）。
        conn.execute_batch(
            "INSERT INTO workspaces (id, root_path, color_tag, created_at, is_active)
             SELECT lower(hex(randomblob(16))), value, 'paper-yellow',
                    strftime('%s', 'now') * 1000, 1
             FROM app_settings
             WHERE key = 'workspace_root'
               AND NOT EXISTS (SELECT 1 FROM workspaces);",
        )
        .unwrap();
        let after: i64 = conn
            .query_row("SELECT count(*) FROM workspaces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, after, "placeholder INSERT must be idempotent");
    }

    #[test]
    fn workspaces_placeholder_inserted_from_legacy_workspace_root() {
        // 存量迁移：先建库到 0015（含 app_settings），写入 workspace_root，
        // 再执行 0016，验证占位行从 app_settings 迁入 workspaces 表。
        let conn = Connection::open_in_memory().unwrap();
        // 手动跑前 15 个迁移（到 0015），跳过 0016。
        for &(version, name, sql) in MIGRATIONS.iter().filter(|(v, _, _)| *v < 16) {
            conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
            conn.execute_batch(sql).unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
            conn.execute_batch("COMMIT;").unwrap();
            let _ = name;
        }
        conn.execute(
            "INSERT INTO app_settings (key, value, updated_at)
             VALUES ('workspace_root', 'D:\\projects\\demo', 1)",
            [],
        )
        .unwrap();
        // 执行 0016 SQL。
        let sql_0016 = MIGRATIONS
            .iter()
            .find(|(v, _, _)| *v == 16)
            .map(|(_, _, s)| *s)
            .unwrap();
        conn.execute_batch(sql_0016).unwrap();
        // 占位行应已从 app_settings 迁入。
        let count: i64 = conn
            .query_row("SELECT count(*) FROM workspaces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 1,
            "placeholder row must be inserted from legacy root"
        );
        let (root, active): (String, i64) = conn
            .query_row("SELECT root_path, is_active FROM workspaces", [], |r| {
                let root: String = r.get(0)?;
                let active: i64 = r.get(1)?;
                Ok((root, active))
            })
            .unwrap();
        assert_eq!(root, "D:\\projects\\demo");
        assert_eq!(active, 1);
    }

    #[test]
    fn sessions_workspace_id_column_and_index() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        // 列存在：INSERT 不指定 workspace_id 应成功（列存在且有默认值）。
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES ('s1','','1','1')",
            [],
        )
        .unwrap();
        // 默认值为 '__migrated__'。
        let wid: String = conn
            .query_row("SELECT workspace_id FROM sessions WHERE id='s1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(wid, "__migrated__", "default must be '__migrated__'");
        // NOT NULL 约束：显式插入 NULL 必须失败。
        let null_err = conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at, workspace_id)
             VALUES ('s2','','1','1', NULL)",
            [],
        );
        assert!(null_err.is_err(), "workspace_id must be NOT NULL");
        // 索引存在。
        let idx: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='idx_sessions_workspace'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx, 1, "idx_sessions_workspace must exist");
    }

    // ---- migration 0026: schedules workspace isolation (task 7.5)

    #[test]
    fn schedules_has_workspace_id_column_and_composite_unique() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();

        // workspace_id column exists with default '__migrated__'
        conn.execute(
            "INSERT INTO schedules (id, name, cron_expr, task_title, created_at, updated_at)
             VALUES ('s1', 'test', '@every 60', 'tick', 1, 1)",
            [],
        )
        .unwrap();
        let wid: String = conn
            .query_row(
                "SELECT workspace_id FROM schedules WHERE id='s1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(wid, "__migrated__");

        // Composite unique index exists
        let idx: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='idx_schedules_new_workspace_name'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx, 1, "composite unique index must exist");
    }

    #[test]
    fn schedules_same_name_different_workspaces_allowed() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();

        // Same name in different workspaces → both succeed
        conn.execute(
            "INSERT INTO schedules (id, name, cron_expr, task_title, created_at, updated_at, workspace_id)
             VALUES ('s1', 'daily', '@every 60', 'tick', 1, 1, 'ws-a')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO schedules (id, name, cron_expr, task_title, created_at, updated_at, workspace_id)
             VALUES ('s2', 'daily', '@every 60', 'tick', 1, 1, 'ws-b')",
            [],
        )
        .unwrap();

        // Same name in same workspace → fails
        let dup = conn.execute(
            "INSERT INTO schedules (id, name, cron_expr, task_title, created_at, updated_at, workspace_id)
             VALUES ('s3', 'daily', '@every 60', 'tick', 1, 1, 'ws-a')",
            [],
        );
        assert!(dup.is_err(), "same name in same workspace must be rejected");
    }

    #[test]
    fn migration_0026_preserves_existing_data() {
        let conn = Connection::open_in_memory().unwrap();

        // Run migrations up to 0025 (skip 0026)
        for &(version, name, sql) in MIGRATIONS.iter().filter(|(v, _, _)| *v < 26) {
            conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
            conn.execute_batch(sql).unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
            conn.execute_batch("COMMIT;").unwrap();
            let _ = name;
        }

        // Insert a schedule before 0026 (no workspace_id column yet)
        conn.execute(
            "INSERT INTO schedules (id, name, cron_expr, task_title, created_at, updated_at)
             VALUES ('s-old', 'nightly', '@every 3600', 'build', 1, 1)",
            [],
        )
        .unwrap();

        // Run migration 0026
        let sql_0026 = MIGRATIONS
            .iter()
            .find(|(v, _, _)| *v == 26)
            .map(|(_, _, s)| *s)
            .unwrap();
        conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
        conn.execute_batch(sql_0026).unwrap();
        conn.pragma_update(None, "user_version", 26).unwrap();
        conn.execute_batch("COMMIT;").unwrap();

        // Verify data preserved with __migrated__ placeholder
        let (name, wid): (String, String) = conn
            .query_row(
                "SELECT name, workspace_id FROM schedules WHERE id='s-old'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "nightly");
        assert_eq!(wid, "__migrated__");
    }

    #[test]
    fn migration_0026_mid_crash_recovery() {
        let conn = Connection::open_in_memory().unwrap();

        // Run migrations up to 0025 (skip 0026) — simulates a DB from before 0026
        for &(version, name, sql) in MIGRATIONS.iter().filter(|(v, _, _)| *v < 26) {
            conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
            conn.execute_batch(sql).unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
            conn.execute_batch("COMMIT;").unwrap();
            let _ = name;
        }

        // Insert a schedule before 0026
        conn.execute(
            "INSERT INTO schedules (id, name, cron_expr, task_title, created_at, updated_at)
             VALUES ('s1', 'nightly', '@every 3600', 'build', 1, 1)",
            [],
        )
        .unwrap();

        // Simulate mid-crash: create a stale schedules_new (as if step 2 ran but
        // the transaction was interrupted before completion)
        conn.execute(
            "CREATE TABLE schedules_new (id TEXT PRIMARY KEY, junk TEXT)",
            [],
        )
        .unwrap();

        // Re-run 0026 SQL — should clean up schedules_new and complete
        let sql_0026 = MIGRATIONS
            .iter()
            .find(|(v, _, _)| *v == 26)
            .map(|(_, _, s)| *s)
            .unwrap();
        conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
        conn.execute_batch(sql_0026).unwrap();
        conn.pragma_update(None, "user_version", 26).unwrap();
        conn.execute_batch("COMMIT;").unwrap();

        // Verify workspace_id column exists
        let col_exists: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('schedules') WHERE name='workspace_id'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            col_exists, 1,
            "workspace_id column must exist after recovery"
        );

        // Verify data preserved
        let (name, wid): (String, String) = conn
            .query_row(
                "SELECT name, workspace_id FROM schedules WHERE id='s1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "nightly");
        assert_eq!(wid, "__migrated__");

        // No stale schedules_new left
        let stale: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='schedules_new'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stale, 0, "schedules_new must be gone after recovery");
    }
}
