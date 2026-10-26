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
    (
        27,
        "0027_steward_sessions",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0027_steward_sessions.sql"
        )),
    ),
    (
        28,
        "0028_steward_ai",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0028_steward_ai.sql"
        )),
    ),
    (
        29,
        "0029_evolution_cycles",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0029_evolution_cycles.sql"
        )),
    ),
    (
        30,
        "0030_evolution_gate_and_pool",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0030_evolution_gate_and_pool.sql"
        )),
    ),
    (
        31,
        "0031_steward_builtin_roles",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0031_steward_builtin_roles.sql"
        )),
    ),
    (
        32,
        "0032_memory_role_binding",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0032_memory_role_binding.sql"
        )),
    ),
    (
        33,
        "0033_event_rollbacks",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0033_event_rollbacks.sql"
        )),
    ),
    (
        34,
        "0034_steward_settings_schema",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0034_steward_settings_schema.sql"
        )),
    ),
    (
        35,
        "0035_steward_main_role_seed",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0035_steward_main_role_seed.sql"
        )),
    ),
    (
        36,
        "0036_work_directories",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0036_work_directories.sql"
        )),
    ),
    (
        37,
        "0037_sandbox_tasks",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0037_sandbox_tasks.sql"
        )),
    ),
    (
        38,
        "0038_sandbox_task_usage",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0038_sandbox_task_usage.sql"
        )),
    ),
    (
        39,
        "0039_proposals",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0039_proposals.sql"
        )),
    ),
    (
        40,
        "0040_session_sources_auth",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0040_session_sources_auth.sql"
        )),
    ),
    (
        41,
        "0041_workspace_name",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../migrations/0041_workspace_name.sql"
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
            "proposals",
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

    // ---- migration 0027-0031: steward AI tables (T1-13 idempotency)

    /// Runs migrations up to and including a target version by manually
    /// executing each SQL batch and updating user_version.
    fn run_migrations_to(conn: &Connection, target: i64) {
        let mut current: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        for &(version, name, sql) in MIGRATIONS {
            if version <= current || version > target {
                continue;
            }
            conn.execute_batch("BEGIN IMMEDIATE;").unwrap();
            conn.execute_batch(sql).unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
            conn.execute_batch("COMMIT;").unwrap();
            current = version;
            let _ = name;
        }
    }

    #[test]
    fn migrations_steward_tables_idempotent_on_empty_db() {
        let conn = Connection::open_in_memory().unwrap();
        // First run: applies all migrations 0001-0039
        run(&conn).unwrap();
        let v1: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v1, MIGRATIONS.last().unwrap().0);

        // Second run: no-op (user_version already at head)
        run(&conn).unwrap();
        let v2: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v2, MIGRATIONS.last().unwrap().0);

        // Steward tables exist and are empty (except roles331 seeds 5 builtin roles)
        let steward_sessions: i64 = conn
            .query_row("SELECT count(*) FROM steward_sessions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(steward_sessions, 0);
        let builtin_roles: i64 = conn
            .query_row(
                "SELECT count(*) FROM roles WHERE id LIKE 'role_steward_%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(builtin_roles, 6);
    }

    #[test]
    fn migrations_0027_0031_preserve_existing_data() {
        let conn = Connection::open_in_memory().unwrap();
        // Run migrations up to 0026 (pre-steward)
        run_migrations_to(&conn, 26);

        // Insert existing data that must survive 0027-0031
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at, kind)
             VALUES ('pre-steward', 'existing', 1, 1, 'chat')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO roles (id, name, created_at, updated_at)
             VALUES ('role_user_1', 'UserRole', 1, 1)",
            [],
        )
        .unwrap();

        // Now run steward migrations 0027-0031
        run_migrations_to(&conn, 31);

        // Existing data preserved
        let session_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sessions WHERE id = 'pre-steward'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(session_count, 1);

        let user_role: i64 = conn
            .query_row(
                "SELECT count(*) FROM roles WHERE id = 'role_user_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(user_role, 1);

        // Steward tables now exist
        let steward_ai: i64 = conn
            .query_row("SELECT count(*) FROM steward_ai", [], |r| r.get(0))
            .unwrap();
        assert_eq!(steward_ai, 0);

        // 5 builtin steward roles seeded (INSERT OR IGNORE is idempotent)
        let builtin: i64 = conn
            .query_row(
                "SELECT count(*) FROM roles WHERE id LIKE 'role_steward_%' AND builtin = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(builtin, 5);
    }

    // ---- migration 0036: work_directories (ADR-0021 lossless upgrade)

    #[test]
    fn migrations_0036_lossless_upgrade_from_0035() {
        let conn = Connection::open_in_memory().unwrap();
        // Build a pre-0036 database (0035 state) with two workspaces,
        // sessions owned by them, and a layout snapshot.
        run_migrations_to(&conn, 35);
        conn.execute(
            "INSERT INTO workspaces (id, root_path, color_tag, created_at, is_active, is_pinned)
             VALUES ('w1', 'C:\\proj\\alpha', 'paper-yellow', 100, 1, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workspaces (id, root_path, color_tag, created_at, is_active, is_pinned)
             VALUES ('w2', 'C:\\proj\\beta', 'paper-blue', 200, 0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at, workspace_id)
             VALUES ('s1', 'alpha chat', 1, 1, 'w1'),
                    ('s2', 'beta chat', 2, 2, 'w2')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workspace_layout_snapshot (id, mode, focused_workspace_id, captured_at)
             VALUES (1, 'single', 'w1', 1)",
            [],
        )
        .unwrap();

        // Apply 0036 (run continues from user_version 35).
        run(&conn).unwrap();

        // Each workspace has exactly one work directory backfilled from
        // root_path, mounted, with added_at = created_at.
        let dirs: Vec<(String, String, i64, i64)> = conn
            .prepare("SELECT workspace_id, abs_path, mounted, added_at FROM work_directories ORDER BY workspace_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(dirs.len(), 2);
        assert_eq!(
            (dirs[0].0.as_str(), dirs[0].1.as_str(), dirs[0].2),
            ("w1", "C:\\proj\\alpha", 1)
        );
        assert_eq!(dirs[0].3, 100, "added_at inherits workspaces.created_at");
        assert_eq!(
            (dirs[1].0.as_str(), dirs[1].1.as_str(), dirs[1].2),
            ("w2", "C:\\proj\\beta", 1)
        );
        assert_eq!(dirs[1].3, 200);

        // Sessions keep their workspace ownership untouched.
        let owners: Vec<(String, String)> = conn
            .prepare("SELECT id, workspace_id FROM sessions ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            owners,
            vec![("s1".into(), "w1".into()), ("s2".into(), "w2".into())]
        );

        // Layout snapshot untouched.
        let snapshot_mode: String = conn
            .query_row(
                "SELECT mode FROM workspace_layout_snapshot WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(snapshot_mode, "single");

        // workspaces rows untouched (root_path preserved — append-only rule).
        let ws_count: i64 = conn
            .query_row("SELECT count(*) FROM workspaces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ws_count, 2);

        // Idempotent: a second run does not create more directories.
        run(&conn).unwrap();
        let dir_count: i64 = conn
            .query_row("SELECT count(*) FROM work_directories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(dir_count, 2);
    }

    #[test]
    fn migrations_0036_skips_workspaces_with_empty_root_path() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations_to(&conn, 35);
        conn.execute(
            "INSERT INTO workspaces (id, root_path, color_tag, created_at, is_active, is_pinned)
             VALUES ('w-empty', '', 'paper-yellow', 100, 0, 0)",
            [],
        )
        .unwrap();
        run(&conn).unwrap();
        let dir_count: i64 = conn
            .query_row("SELECT count(*) FROM work_directories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(dir_count, 0, "empty root_path yields no work directory");
        // Original anomaly record preserved (skip, not drop — spec 5.5.3-2).
        let ws_count: i64 = conn
            .query_row("SELECT count(*) FROM workspaces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ws_count, 1, "anomalous workspace row must be preserved");
    }

    // ---- migration 0037: sandbox_tasks (ADR-0022 lossless upgrade)

    #[test]
    fn migrations_0037_lossless_upgrade_from_0036() {
        let conn = Connection::open_in_memory().unwrap();
        // Build a 0036-state database with a workspace, a work directory
        // backfilled from root_path, and a session.
        run_migrations_to(&conn, 36);
        conn.execute(
            "INSERT INTO workspaces (id, root_path, color_tag, created_at, is_active, is_pinned)
             VALUES ('w1', 'C:\\proj\\alpha', 'paper-yellow', 100, 1, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO work_directories (id, workspace_id, abs_path, display_name, mounted, added_at)
             VALUES ('d1', 'w1', 'C:\\proj\\alpha', 'alpha', 1, 100)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at, workspace_id)
             VALUES ('s1', 'alpha chat', 1, 1, 'w1')",
            [],
        )
        .unwrap();

        // Apply 0037..latest (run continues from user_version 36).
        run(&conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.last().unwrap().0);

        // Existing tables and rows are untouched (zero change to prior state).
        let ws_count: i64 = conn
            .query_row("SELECT count(*) FROM workspaces", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ws_count, 1);
        let dir_count: i64 = conn
            .query_row("SELECT count(*) FROM work_directories", [], |r| r.get(0))
            .unwrap();
        assert_eq!(dir_count, 1);
        let session_owner: String = conn
            .query_row(
                "SELECT workspace_id FROM sessions WHERE id = 's1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(session_owner, "w1");

        // sandbox_tasks exists and starts empty; a row round-trips with the
        // full 0037 column set (task spec + lifecycle audit fields).
        let empty: i64 = conn
            .query_row("SELECT count(*) FROM sandbox_tasks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(empty, 0);
        conn.execute(
            "INSERT INTO sandbox_tasks (id, task_type, runtime, status, source, authorized_dirs, limits, stdout, stderr, artifact_ref, artifact_retryable, exit_info, duration_ms, created_at, finished_at)
             VALUES ('t1', 'run_code', 'python', 'succeeded', 'print(1)',
                     '[\"C:\\\\proj\\\\alpha\"]', '{\"timeout_secs\":120}', '1\n', '', null, 0, 'code=0', 42, 1000, 1042)",
            [],
        )
        .unwrap();
        let (task_type, status, artifact_retryable, duration_ms): (String, String, i64, i64) = conn
            .query_row(
                "SELECT task_type, status, artifact_retryable, duration_ms FROM sandbox_tasks WHERE id = 't1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (
                task_type.as_str(),
                status.as_str(),
                artifact_retryable,
                duration_ms
            ),
            ("run_code", "succeeded", 0, 42)
        );

        // CHECK constraints reject values outside the enumerated sets.
        let bad_type: Result<usize, _> = conn.execute(
            "INSERT INTO sandbox_tasks (id, task_type, runtime, status, source, authorized_dirs, limits, created_at)
             VALUES ('t2', 'evil', 'python', 'queued', 'x', '[]', '{}', 1)",
            [],
        );
        assert!(
            bad_type.is_err(),
            "task_type CHECK must reject unknown kinds"
        );
        let bad_status: Result<usize, _> = conn.execute(
            "INSERT INTO sandbox_tasks (id, task_type, runtime, status, source, authorized_dirs, limits, created_at)
             VALUES ('t3', 'run_code', 'python', 'paused', 'x', '[]', '{}', 1)",
            [],
        );
        assert!(
            bad_status.is_err(),
            "status CHECK must reject unknown states"
        );

        // Idempotent: a second run applies nothing new (no duplicate effects).
        run(&conn).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM sandbox_tasks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    // ---- task 9.4: full upgrade drill 0035 -> 0037 (lossless, idempotent)

    #[test]
    fn migrations_0035_to_0037_full_upgrade_drill() {
        let conn = Connection::open_in_memory().unwrap();
        // Build a 0035-state database holding every aggregate the drill must
        // verify across the 0036/0037 hop: workspaces (with color tags),
        // sessions, a steward session + its replayable history events, an
        // attachment, a session event, and a layout snapshot.
        run_migrations_to(&conn, 35);
        conn.execute(
            "INSERT INTO workspaces (id, root_path, color_tag, created_at, is_active, is_pinned)
             VALUES ('w1', 'C:\\proj\\alpha', 'paper-yellow', 100, 1, 0),
                    ('w2', 'C:\\proj\\beta', 'paper-blue', 200, 0, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at, kind, workspace_id)
             VALUES ('s1', 'alpha chat', 1, 1, 'chat', 'w1'),
                    ('s2', 'beta chat', 2, 2, 'chat', 'w2'),
                    ('stew-1', 'steward chat', 3, 3, 'background', 'w1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO steward_sessions (id, steward_id, title, goal, created_at, updated_at)
             VALUES ('stew-1', 'steward_main', '管家会话', 'help me', 3, 3)",
            [],
        )
        .unwrap();
        // Steward history: two replayable events on the events log.
        conn.execute(
            "INSERT INTO events (aggregate_type, aggregate_id, kind, payload, seq, created_at)
             VALUES ('steward', 'stew-1', 'message', '{\"role\":\"user\",\"content\":\"hi\"}', 1, 10),
                    ('steward', 'stew-1', 'message', '{\"role\":\"assistant\",\"content\":\"hello\"}', 2, 11)",
            [],
        )
        .unwrap();
        // A regular session event.
        conn.execute(
            "INSERT INTO events (aggregate_type, aggregate_id, kind, payload, seq, created_at)
             VALUES ('session', 's1', 'message', '{\"content\":\"alpha msg\"}', 1, 12)",
            [],
        )
        .unwrap();
        // Attachment metadata.
        conn.execute(
            "INSERT INTO attachments (id, session_id, seq, kind, name, mime, rel_path, size_bytes, sha256, created_at)
             VALUES ('att-1', 'stew-1', NULL, 'image', 'shot.png', 'image/png',
                     '.nuomi/attachments/stew-1/abc.png', 1234, 'abc123', 5)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workspace_layout_snapshot (id, mode, focused_workspace_id, captured_at)
             VALUES (1, 'single', 'w1', 1)",
            [],
        )
        .unwrap();

        // Upgrade: 0036 -> latest in one run.
        run(&conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.last().unwrap().0);

        // (1) Workspace <-> work directory: one backfilled directory per
        // workspace, mounted, added_at = created_at.
        let dirs: Vec<(String, String, i64, i64)> = conn
            .prepare("SELECT workspace_id, abs_path, mounted, added_at FROM work_directories ORDER BY workspace_id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(dirs.len(), 2);
        assert_eq!(
            (dirs[0].0.as_str(), dirs[0].1.as_str(), dirs[0].2, dirs[0].3),
            ("w1", "C:\\proj\\alpha", 1, 100)
        );
        assert_eq!(
            (dirs[1].0.as_str(), dirs[1].1.as_str(), dirs[1].2, dirs[1].3),
            ("w2", "C:\\proj\\beta", 1, 200)
        );

        // (2) Identity intact: color_tag unchanged by the upgrade.
        let colors: Vec<(String, String)> = conn
            .prepare("SELECT id, color_tag FROM workspaces ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            colors,
            vec![
                ("w1".into(), "paper-yellow".into()),
                ("w2".into(), "paper-blue".into())
            ]
        );

        // (3) Session ownership unchanged.
        let owners: Vec<(String, String)> = conn
            .prepare("SELECT id, workspace_id FROM sessions ORDER BY id")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            owners,
            vec![
                ("s1".into(), "w1".into()),
                ("s2".into(), "w2".into()),
                ("stew-1".into(), "w1".into())
            ]
        );

        // (4) Steward session discriminant survives.
        let steward: i64 = conn
            .query_row(
                "SELECT count(*) FROM steward_sessions WHERE id = 'stew-1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(steward, 1);

        // (5) Steward history replayable: both events intact, in seq order,
        // with the original payloads (spec 4.2-4).
        let replay: Vec<(String, String, i64)> = conn
            .prepare(
                "SELECT kind, payload, seq FROM events \
                 WHERE aggregate_type = 'steward' AND aggregate_id = 'stew-1' ORDER BY seq",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            replay,
            vec![
                (
                    "message".into(),
                    "{\"role\":\"user\",\"content\":\"hi\"}".into(),
                    1
                ),
                (
                    "message".into(),
                    "{\"role\":\"assistant\",\"content\":\"hello\"}".into(),
                    2
                ),
            ]
        );

        // (6) Attachment metadata fully preserved (seq NULL = not yet bound).
        let att: (String, String, i64, String, String, Option<i64>, String) = conn
            .query_row(
                "SELECT id, session_id, size_bytes, mime, rel_path, seq, sha256 FROM attachments WHERE id = 'att-1'",
                [],
                |r| {
                    Ok((
                        r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            att,
            (
                "att-1".into(),
                "stew-1".into(),
                1234,
                "image/png".into(),
                ".nuomi/attachments/stew-1/abc.png".into(),
                None,
                "abc123".into()
            )
        );

        // (7) Regular session event still on the log.
        let session_events: i64 = conn
            .query_row(
                "SELECT count(*) FROM events WHERE aggregate_type = 'session' AND aggregate_id = 's1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(session_events, 1);

        // (8) Layout snapshot untouched.
        let snapshot: (String, String) = conn
            .query_row(
                "SELECT mode, focused_workspace_id FROM workspace_layout_snapshot WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(snapshot, ("single".into(), "w1".into()));

        // (9) Idempotent: a second run changes nothing.
        run(&conn).unwrap();
        let (dir_count, att_count, event_count, snapshot_count): (i64, i64, i64, i64) = conn
            .query_row(
                "SELECT
                    (SELECT count(*) FROM work_directories),
                    (SELECT count(*) FROM attachments),
                    (SELECT count(*) FROM events),
                    (SELECT count(*) FROM workspace_layout_snapshot)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (dir_count, att_count, event_count, snapshot_count),
            (2, 1, 3, 1)
        );
    }

    // ---- migration 0040: session sources + authorization (chat_rich_bind)

    #[test]
    fn migrations_0040_lossless_upgrade_from_0039() {
        let conn = Connection::open_in_memory().unwrap();
        // Build a 0039-state database: workspaces + work directory + sessions
        // with events and attachment metadata that must survive 0040.
        run_migrations_to(&conn, 39);
        conn.execute(
            "INSERT INTO workspaces (id, root_path, color_tag, created_at, is_active, is_pinned)
             VALUES ('w1', 'C:\\proj\\alpha', 'paper-yellow', 100, 1, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO work_directories (id, workspace_id, abs_path, display_name, mounted, added_at)
             VALUES ('d1', 'w1', 'C:\\proj\\alpha', 'alpha', 1, 100)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at, kind, workspace_id)
             VALUES ('s1', 'alpha chat', 1, 1, 'chat', 'w1'),
                    ('s2', 'beta chat', 2, 2, 'chat', 'w1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events (aggregate_type, aggregate_id, kind, payload, seq, created_at)
             VALUES ('session', 's1', 'message', '{\"role\":\"assistant\",\"content\":\"hi\"}', 1, 10)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO attachments (id, session_id, seq, kind, name, mime, rel_path, size_bytes, sha256, created_at)
             VALUES ('att-1', 's1', NULL, 'image', 'shot.png', 'image/png',
                     '.nuomi/attachments/s1/abc.png', 1234, 'abc123', 5)",
            [],
        )
        .unwrap();

        // Apply 0040 (run continues from user_version 39).
        run(&conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.last().unwrap().0);

        // Legacy sessions keep work_directory_id NULL (fallback cwd semantics).
        let null_bindings: i64 = conn
            .query_row(
                "SELECT count(*) FROM sessions WHERE work_directory_id IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(null_bindings, 2, "legacy sessions must stay unbound");

        // Existing tables untouched: workspace ownership and events intact.
        let owner: String = conn
            .query_row(
                "SELECT workspace_id FROM sessions WHERE id = 's1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(owner, "w1");
        let event_payload: String = conn
            .query_row(
                "SELECT payload FROM events WHERE aggregate_id = 's1' AND seq = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(event_payload, "{\"role\":\"assistant\",\"content\":\"hi\"}");

        // Attachment metadata fully preserved (attachments table zero change).
        let (kind, mime, sha): (String, String, String) = conn
            .query_row(
                "SELECT kind, mime, sha256 FROM attachments WHERE id = 'att-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (kind.as_str(), mime.as_str(), sha.as_str()),
            ("image", "image/png", "abc123")
        );

        // Seed present with default 120s.
        let timeout: String = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'authorization.timeout_seconds'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(timeout, "120");

        // Idempotent: a second run changes nothing (seed guarded by OR IGNORE).
        run(&conn).unwrap();
        let seed_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM app_settings WHERE key = 'authorization.timeout_seconds'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(seed_count, 1);
    }

    #[test]
    fn migrations_0040_new_tables_constraints_and_cascade() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at, kind, workspace_id)
             VALUES ('s1', 'chat', 1, 1, 'chat', '__migrated__')",
            [],
        )
        .unwrap();

        // file_authorizations round-trips a pending row.
        conn.execute(
            "INSERT INTO file_authorizations (id, session_id, role_id, op, target_path, state, requested_at, expires_at)
             VALUES ('fa1', 's1', 'role_1', 'write', 'C:\\proj\\a.txt', 'pending', 100, 1120)",
            [],
        )
        .unwrap();
        let (op, state, expires_at): (String, String, i64) = conn
            .query_row(
                "SELECT op, state, expires_at FROM file_authorizations WHERE id = 'fa1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (op.as_str(), state.as_str(), expires_at),
            ("write", "pending", 1120)
        );

        // CHECK constraints reject unknown op/state values.
        let bad_op = conn.execute(
            "INSERT INTO file_authorizations (id, session_id, role_id, op, target_path, state, requested_at, expires_at)
             VALUES ('fa2', 's1', 'role_1', 'read', 'C:\\x', 'pending', 1, 2)",
            [],
        );
        assert!(bad_op.is_err(), "op CHECK must reject unknown ops");
        let bad_state = conn.execute(
            "INSERT INTO file_authorizations (id, session_id, role_id, op, target_path, state, requested_at, expires_at)
             VALUES ('fa3', 's1', 'role_1', 'write', 'C:\\x', 'waiting_notified', 1, 2)",
            [],
        );
        assert!(bad_state.is_err(), "state CHECK must reject derived states");

        // Sweep index exists.
        let sweep_idx: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='idx_file_auth_sweep'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sweep_idx, 1, "idx_file_auth_sweep must exist");

        // agent_auth_policies round-trip + CHECK.
        conn.execute(
            "INSERT INTO agent_auth_policies (role_id, policy, updated_at)
             VALUES ('role_1', 'authorize', 100)",
            [],
        )
        .unwrap();
        let bad_policy = conn.execute(
            "INSERT INTO agent_auth_policies (role_id, policy, updated_at)
             VALUES ('role_2', 'unset', 100)",
            [],
        );
        assert!(bad_policy.is_err(), "policy CHECK must reject 'unset'");

        // ON DELETE CASCADE: deleting the session removes its authorizations.
        conn.execute("DELETE FROM sessions WHERE id = 's1'", [])
            .unwrap();
        let remaining: i64 = conn
            .query_row("SELECT count(*) FROM file_authorizations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 0, "authorizations cascade with session");
    }
}
