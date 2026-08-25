//! Team repository: role compositions with a collaboration topology
//! (`teams` table, migration 0001). `topology` is stored as its snake_case
//! string; `member_role_ids` and topology-specific config as JSON TEXT.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{Team, TeamTopology};
use crate::store::StoreError;

pub fn insert(conn: &Connection, t: &Team) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO teams (id, name, topology, member_role_ids, config_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            t.id,
            t.name,
            topology_to_str(t.topology),
            serde_json::to_string(&t.member_role_ids)?,
            serde_json::to_string(&t.config)?,
            t.created_at,
            t.updated_at
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Team, StoreError> {
    conn.query_row(
        "SELECT id, name, topology, member_role_ids, config_json, created_at, updated_at
         FROM teams WHERE id = ?1",
        params![id],
        row_to_team,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "team",
        id: id.to_string(),
    })
}

/// Full-row update of mutable fields; `id` and `created_at` are preserved.
pub fn update(conn: &Connection, t: &Team) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE teams SET name = ?2, topology = ?3, member_role_ids = ?4, config_json = ?5, updated_at = ?6
         WHERE id = ?1",
        params![
            t.id,
            t.name,
            topology_to_str(t.topology),
            serde_json::to_string(&t.member_role_ids)?,
            serde_json::to_string(&t.config)?,
            t.updated_at
        ],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "team",
            id: t.id.clone(),
        });
    }
    Ok(())
}

pub fn list(conn: &Connection) -> Result<Vec<Team>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, topology, member_role_ids, config_json, created_at, updated_at
         FROM teams ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], row_to_team)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Returns whether a row was actually removed.
pub fn delete(conn: &Connection, id: &str) -> Result<bool, StoreError> {
    let n = conn.execute("DELETE FROM teams WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

pub fn count(conn: &Connection) -> Result<i64, StoreError> {
    Ok(conn.query_row("SELECT count(*) FROM teams", [], |row| row.get(0))?)
}

// ---------------------------------------------------------------- helpers

fn topology_to_str(t: TeamTopology) -> &'static str {
    match t {
        TeamTopology::Pipeline => "pipeline",
        TeamTopology::Router => "router",
        TeamTopology::GroupChat => "group_chat",
    }
}

fn row_to_team(row: &rusqlite::Row<'_>) -> rusqlite::Result<Team> {
    let topology: String = row.get(2)?;
    let members: String = row.get(3)?;
    let config_json: String = row.get(4)?;
    let topo = match topology.as_str() {
        "pipeline" => TeamTopology::Pipeline,
        "router" => TeamTopology::Router,
        "group_chat" => TeamTopology::GroupChat,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                format!("unknown topology {other}").into(),
            ))
        }
    };
    Ok(Team {
        id: row.get(0)?,
        name: row.get(1)?,
        topology: topo,
        member_role_ids: crate::store::json_col(0, &members)?,
        config: crate::store::json_col(0, &config_json)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
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

    fn team(id: &str, name: &str) -> Team {
        Team {
            id: id.into(),
            name: name.into(),
            topology: TeamTopology::GroupChat,
            member_role_ids: vec!["r1".into(), "r2".into(), "r3".into()],
            config: serde_json::json!({ "max_rounds": 8, "selector": { "provider": "p1", "temperature": 0.2 } }),
            created_at: 1,
            updated_at: 1,
        }
    }

    fn assert_same(a: &Team, b: &Team) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.name, b.name);
        assert_eq!(a.topology, b.topology);
        assert_eq!(a.member_role_ids, b.member_role_ids);
        assert_eq!(a.config, b.config);
        assert_eq!(a.created_at, b.created_at);
        assert_eq!(a.updated_at, b.updated_at);
    }

    #[test]
    fn insert_get_roundtrip_all_fields() {
        let (_dir, conn) = db();
        let t = team("t1", "crew");
        insert(&conn, &t).unwrap();
        let got = get(&conn, "t1").unwrap();
        assert_same(&got, &t);

        // multi-element membership keeps declaration order exactly
        assert_eq!(
            got.member_role_ids,
            vec!["r1".to_string(), "r2".to_string(), "r3".to_string()]
        );
        // nested config JSON survives losslessly
        assert_eq!(got.config["selector"]["provider"], "p1");

        // unknown id → NotFound
        assert!(matches!(
            get(&conn, "ghost"),
            Err(StoreError::NotFound { entity: "team", .. })
        ));
    }

    #[test]
    fn all_topologies_roundtrip_losslessly() {
        let (_dir, conn) = db();
        for (i, topo) in [
            TeamTopology::Pipeline,
            TeamTopology::Router,
            TeamTopology::GroupChat,
        ]
        .into_iter()
        .enumerate()
        {
            let mut t = team(&format!("t{i}"), &format!("team-{i}"));
            t.topology = topo;
            insert(&conn, &t).unwrap();
            assert_eq!(get(&conn, &format!("t{i}")).unwrap().topology, topo);
        }
    }

    #[test]
    fn rejects_invalid_topology_via_check_constraint() {
        let (_dir, conn) = db();
        assert!(conn
            .execute(
                "INSERT INTO teams (id, name, topology, created_at, updated_at)
                 VALUES ('bad', 'x', 'swarm', 1, 1)",
                [],
            )
            .is_err());
    }

    #[test]
    fn list_orders_by_created_at_asc() {
        let (_dir, conn) = db();
        for (id, at) in [("late", 30), ("early", 10), ("mid", 20)] {
            let mut t = team(id, id);
            t.created_at = at;
            insert(&conn, &t).unwrap();
        }
        let ids: Vec<String> = list(&conn).unwrap().into_iter().map(|t| t.id).collect();
        assert_eq!(ids, vec!["early", "mid", "late"]);
        assert_eq!(count(&conn).unwrap(), 3);
    }

    #[test]
    fn update_overwrites_all_mutable_fields_and_reports_missing() {
        let (_dir, conn) = db();
        let original = team("t1", "old-name");
        insert(&conn, &original).unwrap();

        let mut t = get(&conn, "t1").unwrap();
        t.name = "renamed".into();
        t.topology = TeamTopology::Router;
        t.member_role_ids = vec!["r9".into()];
        t.config = serde_json::json!({ "route_hint": "fastest" });
        t.updated_at = 5;
        update(&conn, &t).unwrap();

        let got = get(&conn, "t1").unwrap();
        assert_same(&got, &t);
        assert_eq!(got.created_at, original.created_at);
        assert_eq!(got.updated_at, 5);

        t.id = "ghost".into();
        assert!(matches!(
            update(&conn, &t),
            Err(StoreError::NotFound { entity: "team", .. })
        ));
    }

    #[test]
    fn delete_reports_presence_then_absence() {
        let (_dir, conn) = db();
        insert(&conn, &team("t1", "one")).unwrap();
        assert!(delete(&conn, "t1").unwrap());
        assert!(!delete(&conn, "t1").unwrap());
        assert_eq!(count(&conn).unwrap(), 0);
    }

    #[test]
    fn duplicate_name_insert_fails_via_unique_constraint() {
        let (_dir, conn) = db();
        insert(&conn, &team("t1", "same-name")).unwrap();
        assert!(insert(&conn, &team("t2", "same-name")).is_err());
    }
}
