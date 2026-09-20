//! CLI agent profile registry (SPEC cli-agents-m1 D7): CRUD over the
//! `agent_profiles` table introduced by migration 0003.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{AgentProfile, CliFlavor};
use crate::store::StoreError;

pub fn insert(conn: &Connection, p: &AgentProfile) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO agent_profiles
         (id, name, adapter, flavor, command, args, env, working_dir, enabled, model_id, resume_args, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            p.id,
            p.name,
            p.adapter,
            p.flavor.as_str(),
            p.command,
            serde_json::to_string(&p.args)?,
            serde_json::to_string(&p.env)?,
            p.working_dir,
            p.enabled as i64,
            p.model_id,
            p.resume_args,
            p.created_at,
            p.updated_at
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<AgentProfile, StoreError> {
    conn.query_row(
        "SELECT id, name, adapter, flavor, command, args, env, working_dir, enabled, model_id, resume_args, created_at, updated_at
         FROM agent_profiles WHERE id = ?1",
        params![id],
        row_to_profile,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "agent_profile",
        id: id.to_string(),
    })
}

/// Full-row update of mutable fields (registry edits).
pub fn update(conn: &Connection, p: &AgentProfile) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE agent_profiles SET name = ?2, adapter = ?3, flavor = ?4, command = ?5,
         args = ?6, env = ?7, working_dir = ?8, enabled = ?9, model_id = ?10, resume_args = ?11, updated_at = ?12
         WHERE id = ?1",
        params![
            p.id,
            p.name,
            p.adapter,
            p.flavor.as_str(),
            p.command,
            serde_json::to_string(&p.args)?,
            serde_json::to_string(&p.env)?,
            p.working_dir,
            p.enabled as i64,
            p.model_id,
            p.resume_args,
            p.updated_at
        ],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "agent_profile",
            id: p.id.clone(),
        });
    }
    Ok(())
}

pub fn list(conn: &Connection) -> Result<Vec<AgentProfile>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, adapter, flavor, command, args, env, working_dir, enabled, model_id, resume_args, created_at, updated_at
         FROM agent_profiles ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], row_to_profile)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Returns whether a row was actually removed.
pub fn delete(conn: &Connection, id: &str) -> Result<bool, StoreError> {
    let n = conn.execute("DELETE FROM agent_profiles WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

pub fn count(conn: &Connection) -> Result<i64, StoreError> {
    Ok(conn.query_row("SELECT count(*) FROM agent_profiles", [], |row| row.get(0))?)
}

// ---------------------------------------------------------------- helpers

fn parse_flavor(col: usize, raw: &str) -> rusqlite::Result<CliFlavor> {
    CliFlavor::parse(raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            col,
            rusqlite::types::Type::Text,
            format!("unknown cli flavor: {raw}").into(),
        )
    })
}

fn row_to_profile(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentProfile> {
    let flavor: String = row.get(3)?;
    let args: String = row.get(5)?;
    let env: String = row.get(6)?;
    Ok(AgentProfile {
        id: row.get(0)?,
        name: row.get(1)?,
        adapter: row.get(2)?,
        flavor: parse_flavor(3, &flavor)?,
        command: row.get(4)?,
        args: crate::store::json_col(0, &args)?,
        env: crate::store::json_col(0, &env)?,
        working_dir: row.get(7)?,
        enabled: row.get::<_, i64>(8)? != 0,
        model_id: row.get(9)?,
        resume_args: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    /// Tempdir-backed SQLite (testing.md rule); `TempDir` must outlive `conn`.
    fn db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("nuomi-test.db")).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrations::run(&conn).unwrap();
        (dir, conn)
    }

    fn profile(id: &str, name: &str) -> AgentProfile {
        AgentProfile {
            id: id.into(),
            name: name.into(),
            adapter: "cli".into(),
            flavor: CliFlavor::ClaudeCode,
            command: "claude".into(),
            args: serde_json::json!(["-p", "{prompt}"]),
            env: serde_json::json!({ "NUOMI": "1" }),
            working_dir: Some("C:/tmp".into()),
            enabled: true,
            model_id: None,
            resume_args: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn assert_same(a: &AgentProfile, b: &AgentProfile) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.name, b.name);
        assert_eq!(a.adapter, b.adapter);
        assert_eq!(a.flavor, b.flavor);
        assert_eq!(a.command, b.command);
        assert_eq!(a.args, b.args);
        assert_eq!(a.env, b.env);
        assert_eq!(a.working_dir, b.working_dir);
        assert_eq!(a.enabled, b.enabled);
        assert_eq!(a.model_id, b.model_id);
        assert_eq!(a.resume_args, b.resume_args);
        assert_eq!(a.created_at, b.created_at);
        assert_eq!(a.updated_at, b.updated_at);
    }

    #[test]
    fn insert_get_roundtrip_all_fields() {
        let (_dir, conn) = db();
        let p = profile("a1", "claude-main");
        insert(&conn, &p).unwrap();
        let got = get(&conn, "a1").unwrap();
        assert_same(&got, &p);

        // unknown id → NotFound
        assert!(matches!(
            get(&conn, "ghost"),
            Err(StoreError::NotFound {
                entity: "agent_profile",
                ..
            })
        ));
    }

    #[test]
    fn list_orders_by_created_at_asc() {
        let (_dir, conn) = db();
        for (id, at) in [("late", 30), ("early", 10), ("mid", 20)] {
            let mut p = profile(id, id);
            p.created_at = at;
            insert(&conn, &p).unwrap();
        }
        let ids: Vec<String> = list(&conn).unwrap().into_iter().map(|p| p.id).collect();
        assert_eq!(ids, vec!["early", "mid", "late"]);
        assert_eq!(count(&conn).unwrap(), 3);
    }

    #[test]
    fn update_edits_fields_and_reports_missing() {
        let (_dir, conn) = db();
        insert(&conn, &profile("a1", "old")).unwrap();

        let mut p = get(&conn, "a1").unwrap();
        p.name = "renamed".into();
        p.flavor = CliFlavor::Codex;
        p.command = "codex".into();
        p.args = serde_json::json!(["exec", "--prompt", "{prompt}"]);
        p.env = serde_json::json!({ "SHELL": "pwsh" });
        p.working_dir = None;
        p.enabled = false;
        p.updated_at = 5;
        update(&conn, &p).unwrap();

        let got = get(&conn, "a1").unwrap();
        assert_same(&got, &p);
        assert!(!got.enabled);
        assert_eq!(got.updated_at, 5);

        p.id = "ghost".into();
        assert!(matches!(
            update(&conn, &p),
            Err(StoreError::NotFound {
                entity: "agent_profile",
                ..
            })
        ));
    }

    #[test]
    fn delete_reports_presence_then_absence() {
        let (_dir, conn) = db();
        insert(&conn, &profile("a1", "one")).unwrap();
        assert!(delete(&conn, "a1").unwrap());
        assert!(!delete(&conn, "a1").unwrap());
        assert_eq!(count(&conn).unwrap(), 0);
    }

    #[test]
    fn duplicate_name_insert_fails_via_unique_constraint() {
        let (_dir, conn) = db();
        insert(&conn, &profile("a1", "same-name")).unwrap();
        assert!(insert(&conn, &profile("a2", "same-name")).is_err());
    }

    #[test]
    fn all_flavors_roundtrip_losslessly() {
        let (_dir, conn) = db();
        for (i, flavor) in [CliFlavor::ClaudeCode, CliFlavor::Codex, CliFlavor::Plain]
            .into_iter()
            .enumerate()
        {
            let mut p = profile(&format!("f{i}"), &format!("agent-{i}"));
            p.flavor = flavor;
            insert(&conn, &p).unwrap();
            assert_eq!(get(&conn, &format!("f{i}")).unwrap().flavor, flavor);
        }
    }

    #[test]
    fn rejects_invalid_flavor_via_check_constraint() {
        let (_dir, conn) = db();
        assert!(conn
            .execute(
                "INSERT INTO agent_profiles
                 (id, name, adapter, flavor, command, created_at, updated_at)
                 VALUES ('bad', 'x', 'cli', 'exploded', 'cmd', 1, 1)",
                [],
            )
            .is_err());
    }
}
