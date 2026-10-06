//! Role repository: behavior overlays over providers (`roles` table,
//! migrations 0001 + 0010). Function-per-operation style over `&Connection`.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::Role;
use crate::store::StoreError;

const COLUMNS: &str =
    "id, name, provider_id, provider_ids, system_prompt_override, tool_allowlist, \
                       required_capabilities, temperature, max_tokens, params_json, builtin, \
                       generated, ephemeral, source_json, created_at, updated_at";

pub fn insert(conn: &Connection, r: &Role) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO roles
         (id, name, provider_id, provider_ids, system_prompt_override, tool_allowlist, \
          required_capabilities, temperature, max_tokens, params_json, builtin, generated, \
          ephemeral, source_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            r.id,
            r.name,
            r.provider_id,
            serde_json::to_string(&r.provider_ids)?,
            r.system_prompt_override,
            serde_json::to_string(&r.tool_allowlist)?,
            serde_json::to_string(&r.required_capabilities)?,
            r.temperature,
            r.max_tokens,
            serde_json::to_string(&r.params)?,
            r.builtin as i64,
            r.generated as i64,
            r.ephemeral as i64,
            r.source.as_ref().map(serde_json::to_string).transpose()?,
            r.created_at,
            r.updated_at
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Role, StoreError> {
    let sql = format!("SELECT {COLUMNS} FROM roles WHERE id = ?1");
    conn.query_row(&sql, params![id], row_to_role)
        .optional()?
        .ok_or_else(|| StoreError::NotFound {
            entity: "role",
            id: id.to_string(),
        })
}

/// Full-row update of mutable fields; `id` and `created_at` are preserved.
pub fn update(conn: &Connection, r: &Role) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE roles SET name = ?2, provider_id = ?3, provider_ids = ?4, system_prompt_override = ?5,
         tool_allowlist = ?6, required_capabilities = ?7, temperature = ?8, max_tokens = ?9,
         params_json = ?10, builtin = ?11, generated = ?12, ephemeral = ?13, source_json = ?14,
         updated_at = ?15
         WHERE id = ?1",
        params![
            r.id,
            r.name,
            r.provider_id,
            serde_json::to_string(&r.provider_ids)?,
            r.system_prompt_override,
            serde_json::to_string(&r.tool_allowlist)?,
            serde_json::to_string(&r.required_capabilities)?,
            r.temperature,
            r.max_tokens,
            serde_json::to_string(&r.params)?,
            r.builtin as i64,
            r.generated as i64,
            r.ephemeral as i64,
            r
                .source
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
            r.updated_at
        ],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "role",
            id: r.id.clone(),
        });
    }
    Ok(())
}

pub fn list(conn: &Connection) -> Result<Vec<Role>, StoreError> {
    let sql = format!("SELECT {COLUMNS} FROM roles ORDER BY created_at ASC");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], row_to_role)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Returns whether a row was actually removed.
pub fn delete(conn: &Connection, id: &str) -> Result<bool, StoreError> {
    let n = conn.execute("DELETE FROM roles WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

/// Deletes every ephemeral (capability-router temp) role; returns the count
/// removed. Used as the run-end GC hook.
pub fn delete_ephemeral(conn: &Connection) -> Result<usize, StoreError> {
    let n = conn.execute("DELETE FROM roles WHERE ephemeral = 1", [])?;
    Ok(n)
}

pub fn count(conn: &Connection) -> Result<i64, StoreError> {
    Ok(conn.query_row("SELECT count(*) FROM roles", [], |row| row.get(0))?)
}

// ---------------------------------------------------------------- helpers

fn row_to_role(row: &rusqlite::Row<'_>) -> rusqlite::Result<Role> {
    let provider_ids: String = row.get(3)?;
    let allowlist: String = row.get(5)?;
    let required: String = row.get(6)?;
    let params_json: String = row.get(9)?;
    let source_json: Option<String> = row.get(13)?;
    Ok(Role {
        id: row.get(0)?,
        name: row.get(1)?,
        provider_id: row.get(2)?,
        provider_ids: crate::store::json_col(0, &provider_ids)?,
        system_prompt_override: row.get(4)?,
        tool_allowlist: crate::store::json_col(0, &allowlist)?,
        required_capabilities: crate::store::json_col(0, &required)?,
        temperature: row.get(7)?,
        max_tokens: row.get(8)?,
        params: crate::store::json_col(0, &params_json)?,
        builtin: row.get::<_, i64>(10)? != 0,
        generated: row.get::<_, i64>(11)? != 0,
        ephemeral: row.get::<_, i64>(12)? != 0,
        source: match source_json {
            Some(json) => Some(crate::store::json_col(0, &json)?),
            None => None,
        },
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Capability;
    use crate::domain::Team;
    use crate::domain::TeamTopology;
    use crate::store::migrations;
    use crate::store::repos::teams;

    /// Tempdir-backed SQLite (testing.md rule); `TempDir` must outlive `conn`.
    fn db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("nuomi-test.db")).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrations::run(&conn).unwrap();
        (dir, conn)
    }

    fn role(id: &str, name: &str) -> Role {
        Role {
            id: id.into(),
            name: name.into(),
            // None by default: provider_id carries a DB-level FK to
            // provider_configs, so a Some(..) fixture needs a real row.
            provider_id: None,
            provider_ids: vec![],
            system_prompt_override: Some("you are...".into()),
            tool_allowlist: vec!["fs.read".into(), "fs.write".into()],
            required_capabilities: vec![Capability::Reasoning],
            temperature: Some(0.7),
            max_tokens: Some(1024),
            params: serde_json::json!({ "top_p": 0.9 }),
            builtin: false,
            generated: false,
            ephemeral: false,
            source: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn assert_same(a: &Role, b: &Role) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.name, b.name);
        assert_eq!(a.provider_id, b.provider_id);
        assert_eq!(a.provider_ids, b.provider_ids);
        assert_eq!(a.system_prompt_override, b.system_prompt_override);
        assert_eq!(a.tool_allowlist, b.tool_allowlist);
        assert_eq!(a.required_capabilities, b.required_capabilities);
        assert_eq!(a.temperature, b.temperature);
        assert_eq!(a.max_tokens, b.max_tokens);
        assert_eq!(a.params, b.params);
        assert_eq!(a.builtin, b.builtin);
        assert_eq!(a.generated, b.generated);
        assert_eq!(a.ephemeral, b.ephemeral);
        assert_eq!(a.source, b.source);
        assert_eq!(a.created_at, b.created_at);
        assert_eq!(a.updated_at, b.updated_at);
    }

    #[test]
    fn insert_get_roundtrip_all_fields() {
        let (_dir, conn) = db();
        // a real provider row: provider_id has a FK into provider_configs
        conn.execute(
            "INSERT INTO provider_configs (id, name, protocol, base_url, created_at, updated_at)
             VALUES ('p1', 'p', 'openai_compatible', 'http://localhost', 1, 1)",
            [],
        )
        .unwrap();
        let r = Role {
            provider_id: Some("p1".into()),
            provider_ids: vec!["p1".into()],
            builtin: true,
            generated: true,
            ephemeral: false,
            source: Some(serde_json::json!({ "description": "weekly report" })),
            required_capabilities: vec![Capability::Reasoning, Capability::Image],
            ..role("r1", "coder")
        };
        insert(&conn, &r).unwrap();
        let got = get(&conn, "r1").unwrap();
        assert_same(&got, &r);

        // nullable / default columns survive as None/empty
        let sparse = role("r2", "bare");
        insert(&conn, &sparse).unwrap();
        let got = get(&conn, "r2").unwrap();
        assert_same(&got, &sparse);

        // unknown id → NotFound
        assert!(matches!(
            get(&conn, "ghost"),
            Err(StoreError::NotFound { entity: "role", .. })
        ));
    }

    #[test]
    fn list_orders_by_created_at_asc() {
        let (_dir, conn) = db();
        for (id, at) in [("late", 30), ("early", 10), ("mid", 20)] {
            let mut r = role(id, id);
            r.created_at = at;
            insert(&conn, &r).unwrap();
        }
        let ids: Vec<String> = list(&conn)
            .unwrap()
            .into_iter()
            .filter(|r| !r.id.starts_with("role_steward_"))
            .map(|r| r.id)
            .collect();
        assert_eq!(ids, vec!["early", "mid", "late"]);
        assert_eq!(count(&conn).unwrap(), 3 + 5);
    }

    #[test]
    fn update_overwrites_all_mutable_fields_and_reports_missing() {
        let (_dir, conn) = db();
        conn.execute(
            "INSERT INTO provider_configs (id, name, protocol, base_url, created_at, updated_at)
             VALUES ('p1', 'p', 'openai_compatible', 'http://localhost', 1, 1)",
            [],
        )
        .unwrap();
        let original = role("r1", "old-name");
        insert(&conn, &original).unwrap();

        let mut r = get(&conn, "r1").unwrap();
        r.name = "renamed".into();
        r.provider_id = Some("p1".into());
        r.provider_ids = vec!["p1".into()];
        r.system_prompt_override = None;
        r.tool_allowlist = vec![];
        r.required_capabilities = vec![];
        r.temperature = None;
        r.max_tokens = None;
        r.params = serde_json::json!({});
        r.builtin = true;
        r.ephemeral = true;
        r.source = None;
        r.updated_at = 5;
        update(&conn, &r).unwrap();

        let got = get(&conn, "r1").unwrap();
        assert_same(&got, &r);
        assert_eq!(got.created_at, original.created_at);
        assert_eq!(got.updated_at, 5);

        r.id = "ghost".into();
        assert!(matches!(
            update(&conn, &r),
            Err(StoreError::NotFound { entity: "role", .. })
        ));
    }

    #[test]
    fn delete_reports_presence_then_absence() {
        let (_dir, conn) = db();
        insert(&conn, &role("r1", "one")).unwrap();
        assert!(delete(&conn, "r1").unwrap());
        assert!(!delete(&conn, "r1").unwrap());
        assert_eq!(count(&conn).unwrap(), 5);
    }

    #[test]
    fn delete_ephemeral_only_touches_temp_roles() {
        let (_dir, conn) = db();
        let mut temp = role("t1", "temp-reasoning-abc");
        temp.ephemeral = true;
        insert(&conn, &temp).unwrap();
        insert(&conn, &role("r1", "permanent")).unwrap();
        assert_eq!(delete_ephemeral(&conn).unwrap(), 1);
        assert!(get(&conn, "r1").is_ok());
        assert!(get(&conn, "t1").is_err());
        assert_eq!(delete_ephemeral(&conn).unwrap(), 0);
    }

    #[test]
    fn duplicate_name_insert_fails_via_unique_constraint() {
        let (_dir, conn) = db();
        insert(&conn, &role("r1", "same-name")).unwrap();
        assert!(insert(&conn, &role("r2", "same-name")).is_err());
    }

    /// Teams reference roles via JSON member ids — no DB-level FK and no
    /// ON DELETE rule — so deleting a referenced role must succeed while the
    /// team row survives untouched.
    #[test]
    fn deleting_role_referenced_by_team_leaves_team_row_intact() {
        let (_dir, conn) = db();
        insert(&conn, &role("r1", "member")).unwrap();
        let team = Team {
            id: "t1".into(),
            name: "crew".into(),
            topology: TeamTopology::Pipeline,
            member_role_ids: vec!["r1".into()],
            config: serde_json::json!({}),
            created_at: 1,
            updated_at: 1,
        };
        teams::insert(&conn, &team).unwrap();

        assert!(delete(&conn, "r1").unwrap());
        let survived = teams::get(&conn, "t1").unwrap();
        assert_eq!(survived.member_role_ids, vec!["r1".to_string()]);
    }
}
