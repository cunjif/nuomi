//! Reference pre-check & two-phase delete service (module C).
//!
//! Scans referencing entities before delete and nullifies references
//! in a single transaction (D5 / ADR 0011).
//! SPEC: docs/specs/settings-integration/spec.md (AC6)

use rusqlite::{params, Connection};

use crate::store::StoreError;

/// Hint that a Role's provider binding is missing (AC6 runtime detection).
#[derive(Debug, Clone)]
pub struct MissingProviderHint {
    pub role_id: String,
    pub role_name: String,
}

/// Detects whether a Role lacks a provider binding and has no
/// `agent_profile_id` fallback. Returns `Some(hint)` when the user should
/// be prompted to configure a provider for this role.
pub fn detect_missing_provider(role: &crate::domain::Role) -> Option<MissingProviderHint> {
    let has_agent_profile = role
        .params
        .get("agent_profile_id")
        .and_then(serde_json::Value::as_str)
        .is_some();
    if role.provider_id.is_none() && !has_agent_profile {
        Some(MissingProviderHint {
            role_id: role.id.clone(),
            role_name: role.name.clone(),
        })
    } else {
        None
    }
}

/// Entities referencing a Provider or Role.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct EntityRefs {
    pub roles: Vec<String>,
    pub teams: Vec<String>,
    pub sessions: Vec<String>,
}

impl EntityRefs {
    pub fn is_empty(&self) -> bool {
        self.roles.is_empty() && self.teams.is_empty() && self.sessions.is_empty()
    }
}

/// Scans for entities referencing a Provider (`roles.provider_id` /
/// `roles.provider_ids`).
pub fn check_provider_refs(conn: &Connection, provider_id: &str) -> Result<EntityRefs, StoreError> {
    let mut refs = EntityRefs::default();

    let mut stmt = conn.prepare("SELECT id FROM roles WHERE provider_id = ?1")?;
    let rows = stmt.query_map(params![provider_id], |row| row.get::<_, String>(0))?;
    for row in rows {
        refs.roles.push(row?);
    }

    let pattern = format!("%\"{}\"%", provider_id);
    let mut stmt = conn.prepare("SELECT id FROM roles WHERE provider_ids LIKE ?1")?;
    let rows = stmt.query_map(params![pattern], |row| row.get::<_, String>(0))?;
    for row in rows {
        let id = row?;
        if !refs.roles.contains(&id) {
            refs.roles.push(id);
        }
    }

    Ok(refs)
}

/// Scans for entities referencing a Role (`teams.member_role_ids` /
/// `conversation_participants`).
pub fn check_role_refs(conn: &Connection, role_id: &str) -> Result<EntityRefs, StoreError> {
    let mut refs = EntityRefs::default();

    let pattern = format!("%\"{}\"%", role_id);
    let mut stmt = conn.prepare("SELECT id FROM teams WHERE member_role_ids LIKE ?1")?;
    let rows = stmt.query_map(params![pattern], |row| row.get::<_, String>(0))?;
    for row in rows {
        refs.teams.push(row?);
    }

    // ADR 0013: 引用检查从 sessions.agent_kind 改为 conversation_participants。
    let mut stmt = conn.prepare(
        "SELECT DISTINCT session_id FROM conversation_participants WHERE agent_kind = 'role' AND agent_ref_id = ?1",
    )?;
    let rows = stmt.query_map(params![role_id], |row| row.get::<_, String>(0))?;
    for row in rows {
        refs.sessions.push(row?);
    }

    Ok(refs)
}

/// Deletes a provider and nullifies all references in a single transaction.
pub fn delete_and_nullify_provider_refs(
    conn: &mut Connection,
    provider_id: &str,
) -> Result<(), StoreError> {
    let tx = conn.transaction()?;

    // Nullify references BEFORE deleting the provider (FK constraint).
    tx.execute(
        "UPDATE roles SET provider_id = NULL WHERE provider_id = ?1",
        params![provider_id],
    )?;

    let pattern = format!("%\"{}\"%", provider_id);
    let role_rows: Vec<(String, String)> = {
        let mut stmt = tx.prepare("SELECT id, provider_ids FROM roles WHERE provider_ids LIKE ?1")?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (role_id, ids_json) in role_rows {
        if let Ok(mut ids) = serde_json::from_str::<Vec<String>>(&ids_json) {
            ids.retain(|x| x != provider_id);
            let new_json = serde_json::to_string(&ids)?;
            tx.execute(
                "UPDATE roles SET provider_ids = ?1 WHERE id = ?2",
                params![new_json, role_id],
            )?;
        }
    }

    tx.execute("DELETE FROM provider_configs WHERE id = ?1", params![provider_id])?;

    tx.commit()?;
    Ok(())
}

/// Deletes a role and nullifies all references in a single transaction.
pub fn delete_and_nullify_role_refs(conn: &mut Connection, role_id: &str) -> Result<(), StoreError> {
    let tx = conn.transaction()?;

    // Nullify references BEFORE deleting the role (FK constraint).
    let pattern = format!("%\"{}\"%", role_id);
    let team_rows: Vec<(String, String)> = {
        let mut stmt = tx.prepare("SELECT id, member_role_ids FROM teams WHERE member_role_ids LIKE ?1")?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (team_id, ids_json) in team_rows {
        if let Ok(mut ids) = serde_json::from_str::<Vec<String>>(&ids_json) {
            ids.retain(|x| x != role_id);
            let new_json = serde_json::to_string(&ids)?;
            tx.execute(
                "UPDATE teams SET member_role_ids = ?1 WHERE id = ?2",
                params![new_json, team_id],
            )?;
        }
    }

    // ADR 0013: 级联清空从 sessions.agent_kind 改为 conversation_participants。
    tx.execute(
        "DELETE FROM conversation_participants WHERE agent_kind = 'role' AND agent_ref_id = ?1",
        params![role_id],
    )?;

    tx.execute("DELETE FROM roles WHERE id = ?1", params![role_id])?;

    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    fn temp_db() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("r.db");
        let conn = rusqlite::Connection::open(&file).unwrap();
        migrations::run(&conn).unwrap();
        (dir, file)
    }

    #[test]
    fn check_provider_refs_finds_role_with_provider_id() {
        let (_dir, file) = temp_db();
        let conn = rusqlite::Connection::open(&file).unwrap();
        let provider = crate::domain::ProviderConfig {
            id: "p1".into(),
            name: "primary".into(),
            protocol: crate::domain::ProviderProtocol::OpenAiCompatible,
            base_url: "http://x".into(),
            keyring_ref: None,
            capabilities: vec![],
            is_master: false,
            fallback_order: None,
            params: serde_json::json!({}),
            created_at: 1,
            updated_at: 1,
        };
        crate::store::repos::providers::insert_provider(&conn, &provider).unwrap();
        let role = crate::domain::Role {
            id: "r1".into(),
            name: "coder".into(),
            provider_id: Some("p1".into()),
            provider_ids: vec![],
            system_prompt_override: None,
            tool_allowlist: vec![],
            required_capabilities: vec![],
            temperature: None,
            max_tokens: None,
            params: serde_json::json!({}),
            builtin: false,
            generated: false,
            ephemeral: false,
            source: None,
            created_at: 1,
            updated_at: 1,
        };
        crate::store::repos::roles::insert(&conn, &role).unwrap();

        let refs = check_provider_refs(&conn, "p1").unwrap();
        assert_eq!(refs.roles, vec!["r1".to_string()]);
        assert!(refs.is_empty() == false);
    }

    #[test]
    fn check_provider_refs_finds_role_with_provider_ids_array() {
        let (_dir, file) = temp_db();
        let conn = rusqlite::Connection::open(&file).unwrap();
        let role = crate::domain::Role {
            id: "r1".into(),
            name: "coder".into(),
            provider_id: None,
            provider_ids: vec!["p1".into(), "p2".into()],
            system_prompt_override: None,
            tool_allowlist: vec![],
            required_capabilities: vec![],
            temperature: None,
            max_tokens: None,
            params: serde_json::json!({}),
            builtin: false,
            generated: false,
            ephemeral: false,
            source: None,
            created_at: 1,
            updated_at: 1,
        };
        crate::store::repos::roles::insert(&conn, &role).unwrap();

        let refs = check_provider_refs(&conn, "p1").unwrap();
        assert!(refs.roles.contains(&"r1".to_string()));
    }

    #[test]
    fn check_provider_refs_empty_when_no_refs() {
        let (_dir, file) = temp_db();
        let conn = rusqlite::Connection::open(&file).unwrap();
        let refs = check_provider_refs(&conn, "p1").unwrap();
        assert!(refs.is_empty());
    }

    #[test]
    fn delete_and_nullify_provider_removes_refs() {
        let (_dir, file) = temp_db();
        let mut conn = rusqlite::Connection::open(&file).unwrap();
        let provider = crate::domain::ProviderConfig {
            id: "p1".into(),
            name: "primary".into(),
            protocol: crate::domain::ProviderProtocol::OpenAiCompatible,
            base_url: "http://x".into(),
            keyring_ref: None,
            capabilities: vec![],
            is_master: false,
            fallback_order: None,
            params: serde_json::json!({}),
            created_at: 1,
            updated_at: 1,
        };
        crate::store::repos::providers::insert_provider(&conn, &provider).unwrap();
        let role = crate::domain::Role {
            id: "r1".into(),
            name: "coder".into(),
            provider_id: Some("p1".into()),
            provider_ids: vec!["p1".into()],
            system_prompt_override: None,
            tool_allowlist: vec![],
            required_capabilities: vec![],
            temperature: None,
            max_tokens: None,
            params: serde_json::json!({}),
            builtin: false,
            generated: false,
            ephemeral: false,
            source: None,
            created_at: 1,
            updated_at: 1,
        };
        crate::store::repos::roles::insert(&conn, &role).unwrap();

        delete_and_nullify_provider_refs(&mut conn, "p1").unwrap();

        let refs = check_provider_refs(&conn, "p1").unwrap();
        assert!(refs.is_empty());
        let updated_role = crate::store::repos::roles::get(&conn, "r1").unwrap();
        assert!(updated_role.provider_id.is_none());
        assert!(updated_role.provider_ids.is_empty());
    }
}
