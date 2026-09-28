//! Provider repository (`provider_configs`, migration 0001). Role/Team CRUD
//! lives in `repos::roles` / `repos::teams`.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{ProviderConfig, ProviderProtocol};
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

/// Updates an existing row in place (id/created_at preserved). The name
/// column carries a UNIQUE constraint, so renaming onto an existing name
/// fails here just like on insert.
pub fn update_provider(conn: &Connection, p: &ProviderConfig) -> Result<(), StoreError> {
    let changed = conn.execute(
        "UPDATE provider_configs
         SET name = ?2, protocol = ?3, base_url = ?4, keyring_ref = ?5, capabilities = ?6,
             is_master = ?7, fallback_order = ?8, params_json = ?9, updated_at = ?10
         WHERE id = ?1",
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
            p.updated_at
        ],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound {
            entity: "provider_config",
            id: p.id.clone(),
        });
    }
    Ok(())
}

/// Deletes a row; `false` when the id does not exist. A `ForeignKey`
/// failure means roles/teams still reference this provider — the caller
/// surfaces that instead of cascading.
pub fn delete_provider(conn: &Connection, id: &str) -> Result<bool, StoreError> {
    let changed = conn.execute("DELETE FROM provider_configs WHERE id = ?1", params![id])?;
    Ok(changed > 0)
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
        ProviderProtocol::SenseNova => "sense_nova",
    }
}

fn protocol_from_str(s: &str) -> Option<ProviderProtocol> {
    match s {
        "openai_compatible" => Some(ProviderProtocol::OpenAiCompatible),
        "anthropic_compatible" => Some(ProviderProtocol::AnthropicCompatible),
        "sense_nova" => Some(ProviderProtocol::SenseNova),
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
    fn update_provider_preserves_id_and_created_at() {
        let conn = db();
        insert_provider(&conn, &provider("p1")).unwrap();
        let mut edited = provider("p1");
        edited.name = "renamed".into();
        edited.base_url = "http://elsewhere".into();
        edited.params =
            crate::domain::entities::ProviderSettings::default().into_params(edited.params);
        edited.updated_at = 99;
        update_provider(&conn, &edited).unwrap();
        let got = get_provider(&conn, "p1").unwrap();
        assert_eq!(got.name, "renamed");
        assert_eq!(got.base_url, "http://elsewhere");
        assert_eq!(got.created_at, 1);
        assert_eq!(got.updated_at, 99);
        assert_eq!(
            crate::domain::entities::ProviderSettings::from_params(&got.params),
            crate::domain::entities::ProviderSettings::default()
        );
    }

    #[test]
    fn update_missing_provider_is_not_found() {
        let conn = db();
        assert!(update_provider(&conn, &provider("ghost")).is_err());
    }

    #[test]
    fn delete_provider_reports_existence() {
        let conn = db();
        insert_provider(&conn, &provider("p1")).unwrap();
        assert!(delete_provider(&conn, "p1").unwrap());
        assert!(!delete_provider(&conn, "p1").unwrap());
        assert!(get_provider(&conn, "p1").is_err());
    }
}
