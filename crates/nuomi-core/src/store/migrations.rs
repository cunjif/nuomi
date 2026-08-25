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
}
