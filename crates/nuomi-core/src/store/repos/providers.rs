//! Provider / Role / Team repositories.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{ProviderConfig, ProviderProtocol, Role, Team, TeamTopology};
use crate::store::StoreError;

// ---------- ProviderConfig ----------

pub fn insert_provider(conn: &Connection, p: &ProviderConfig) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO provider_configs
         (id, name, protocol, base_url, keyring_ref, capabilities, is_master, fallback_order, params_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            p.id,
            p.name,
            protocol_to_str(p.protocol),
            p.base_url,
            p.keyring_ref,
            serde_json::to_string(&p.capabilities)?,
            p.is_master as i64,
            p.fallback_order,
            serde_json::to_string(&p.params)?,
            p.created_at,
            p.updated_at
        ],
    )?;
    Ok(())
}

pub fn list_providers(conn: &Connection) -> Result<Vec<ProviderConfig>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, protocol, base_url, keyring_ref, capabilities, is_master, fallback_order, params_json, created_at, updated_at
         FROM provider_configs ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], row_provider)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get_provider(conn: &Connection, id: &str) -> Result<ProviderConfig, StoreError> {
    conn.query_row(
        "SELECT id, name, protocol, base_url, keyring_ref, capabilities, is_master, fallback_order, params_json, created_at, updated_at
         FROM provider_configs WHERE id = ?1",
        params![id],
        row_provider,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound { entity: "provider_config", id: id.to_string() })
}

fn protocol_to_str(p: ProviderProtocol) -> &'static str {
    match p {
        ProviderProtocol::OpenAiCompatible => "openai_compatible",
        ProviderProtocol::AnthropicCompatible => "anthropic_compatible",
    }
}

fn protocol_from_str(s: &str) -> Option<ProviderProtocol> {
    match s {
        "openai_compatible" => Some(ProviderProtocol::OpenAiCompatible),
        "anthropic_compatible" => Some(ProviderProtocol::AnthropicCompatible),
        _ => None,
    }
}

fn row_provider(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProviderConfig> {
    let protocol: String = row.get(2)?;
    let capabilities: String = row.get(5)?;
    let params_json: String = row.get(8)?;
    Ok(ProviderConfig {
        id: row.get(0)?,
        name: row.get(1)?,
        protocol: protocol_from_str(&protocol).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                format!("unknown protocol {protocol}").into(),
            )
        })?,
        base_url: row.get(3)?,
        keyring_ref: row.get(4)?,
        capabilities: crate::store::json_col(0, &capabilities)?,
        is_master: row.get::<_, i64>(6)? != 0,
        fallback_order: row.get(7)?,
        params: crate::store::json_col(0, &params_json)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

// ---------- Role ----------

pub fn insert_role(conn: &Connection, r: &Role) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO roles
         (id, name, provider_id, system_prompt_override, tool_allowlist, temperature, max_tokens, params_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            r.id,
            r.name,
            r.provider_id,
            r.system_prompt_override,
            serde_json::to_string(&r.tool_allowlist)?,
            r.temperature,
            r.max_tokens,
            serde_json::to_string(&r.params)?,
            r.created_at,
            r.updated_at
        ],
    )?;
    Ok(())
}

pub fn get_role(conn: &Connection, id: &str) -> Result<Role, StoreError> {
    conn.query_row(
        "SELECT id, name, provider_id, system_prompt_override, tool_allowlist, temperature, max_tokens, params_json, created_at, updated_at
         FROM roles WHERE id = ?1",
        params![id],
        row_role,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound { entity: "role", id: id.to_string() })
}

fn row_role(row: &rusqlite::Row<'_>) -> rusqlite::Result<Role> {
    let allowlist: String = row.get(4)?;
    let params_json: String = row.get(7)?;
    Ok(Role {
        id: row.get(0)?,
        name: row.get(1)?,
        provider_id: row.get(2)?,
        system_prompt_override: row.get(3)?,
        tool_allowlist: crate::store::json_col(0, &allowlist)?,
        temperature: row.get(5)?,
        max_tokens: row.get(6)?,
        params: crate::store::json_col(0, &params_json)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

// ---------- Team ----------

pub fn insert_team(conn: &Connection, t: &Team) -> Result<(), StoreError> {
    let topology = match t.topology {
        TeamTopology::Pipeline => "pipeline",
        TeamTopology::Router => "router",
        TeamTopology::GroupChat => "group_chat",
    };
    conn.execute(
        "INSERT INTO teams (id, name, topology, member_role_ids, config_json, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            t.id,
            t.name,
            topology,
            serde_json::to_string(&t.member_role_ids)?,
            serde_json::to_string(&t.config)?,
            t.created_at,
            t.updated_at
        ],
    )?;
    Ok(())
}

pub fn get_team(conn: &Connection, id: &str) -> Result<Team, StoreError> {
    conn.query_row(
        "SELECT id, name, topology, member_role_ids, config_json, created_at, updated_at
         FROM teams WHERE id = ?1",
        params![id],
        row_team,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "team",
        id: id.to_string(),
    })
}

fn row_team(row: &rusqlite::Row<'_>) -> rusqlite::Result<Team> {
    let topology: String = row.get(2)?;
    let members: String = row.get(3)?;
    let config: String = row.get(4)?;
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
        config: crate::store::json_col(0, &config)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
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

    fn provider(id: &str) -> ProviderConfig {
        ProviderConfig {
            id: id.into(),
            name: format!("p-{id}"),
            protocol: ProviderProtocol::OpenAiCompatible,
            base_url: "http://localhost".into(),
            keyring_ref: Some("kr".into()),
            capabilities: vec!["code".into()],
            is_master: true,
            fallback_order: None,
            params: serde_json::json!({}),
            created_at: 1,
            updated_at: 1,
        }
    }

    fn role(id: &str, provider_id: Option<&str>) -> Role {
        Role {
            id: id.into(),
            name: format!("r-{id}"),
            provider_id: provider_id.map(str::to_string),
            system_prompt_override: Some("you are...".into()),
            tool_allowlist: vec!["fs.read".into()],
            temperature: Some(0.7),
            max_tokens: Some(1024),
            params: serde_json::json!({}),
            created_at: 1,
            updated_at: 1,
        }
    }

    #[test]
    fn provider_roundtrip_with_capabilities() {
        let conn = db();
        insert_provider(&conn, &provider("p1")).unwrap();
        let got = get_provider(&conn, "p1").unwrap();
        assert_eq!(got.capabilities, vec!["code".to_string()]);
        assert!(got.is_master);
        assert_eq!(got.protocol, ProviderProtocol::OpenAiCompatible);
    }

    #[test]
    fn get_missing_provider_is_not_found() {
        let conn = db();
        assert!(get_provider(&conn, "x").is_err());
    }

    #[test]
    fn duplicate_provider_name_fails() {
        let conn = db();
        insert_provider(&conn, &provider("a")).unwrap();
        let mut b = provider("b");
        b.name = "p-a".into();
        assert!(insert_provider(&conn, &b).is_err());
    }

    #[test]
    fn role_roundtrip_and_foreign_key_enforced() {
        let conn = db();
        insert_role(&conn, &role("r1", Some("missing"))).unwrap_err(); // FK on
        insert_provider(&conn, &provider("p1")).unwrap();
        insert_role(&conn, &role("r1", Some("p1"))).unwrap();
        let got = get_role(&conn, "r1").unwrap();
        assert_eq!(got.tool_allowlist, vec!["fs.read".to_string()]);
        assert_eq!(got.temperature, Some(0.7));
    }

    #[test]
    fn team_roundtrip_with_members() {
        let conn = db();
        let team = Team {
            id: "t1".into(),
            name: "crew".into(),
            topology: TeamTopology::GroupChat,
            member_role_ids: vec!["r1".into(), "r2".into()],
            config: serde_json::json!({ "max_rounds": 8 }),
            created_at: 1,
            updated_at: 1,
        };
        insert_team(&conn, &team).unwrap();
        let got = get_team(&conn, "t1").unwrap();
        assert_eq!(got.topology, TeamTopology::GroupChat);
        assert_eq!(got.member_role_ids.len(), 2);
        assert_eq!(got.config["max_rounds"], 8);
    }
}
