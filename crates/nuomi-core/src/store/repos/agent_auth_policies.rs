//! Agent default-authorization-policy repository (`agent_auth_policies`
//! table, migration 0040).
//!
//! Policies are per-RoleAgent GLOBAL settings (spec 6.3-1, not per-session):
//! the Sweeper falls back to them when a pending authorization times out.
//! "unset" is modeled as the ABSENCE of a row — never a magic value.

use rusqlite::{params, Connection, OptionalExtension};

use crate::store::StoreError;

/// A stored policy value ("unset" never materializes here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentAuthPolicyValue {
    Authorize,
    Deny,
}

impl AgentAuthPolicyValue {
    pub const fn as_str(self) -> &'static str {
        match self {
            AgentAuthPolicyValue::Authorize => "authorize",
            AgentAuthPolicyValue::Deny => "deny",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "authorize" => Some(AgentAuthPolicyValue::Authorize),
            "deny" => Some(AgentAuthPolicyValue::Deny),
            _ => None,
        }
    }
}

/// Upserts a policy row (authorize/deny). Errors when the role does not exist
/// (FK pre-check surfaces `NotFound` instead of a raw constraint failure).
pub fn upsert(
    conn: &Connection,
    role_id: &str,
    policy: AgentAuthPolicyValue,
) -> Result<(), StoreError> {
    let role_exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM roles WHERE id = ?1)",
        params![role_id],
        |r| r.get(0),
    )?;
    if !role_exists {
        return Err(StoreError::NotFound {
            entity: "role",
            id: role_id.to_string(),
        });
    }
    conn.execute(
        "INSERT INTO agent_auth_policies (role_id, policy, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(role_id) DO UPDATE SET policy = excluded.policy, updated_at = excluded.updated_at",
        params![role_id, policy.as_str(), crate::domain::now_ms()],
    )?;
    Ok(())
}

/// Removes the policy row — the "unset" semantic.
pub fn unset(conn: &Connection, role_id: &str) -> Result<(), StoreError> {
    conn.execute(
        "DELETE FROM agent_auth_policies WHERE role_id = ?1",
        params![role_id],
    )?;
    Ok(())
}

/// Reads a single role's policy (`None` = unset).
pub fn find_by_role(
    conn: &Connection,
    role_id: &str,
) -> Result<Option<AgentAuthPolicyValue>, StoreError> {
    let value: Option<String> = conn
        .query_row(
            "SELECT policy FROM agent_auth_policies WHERE role_id = ?1",
            params![role_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(value.as_deref().and_then(AgentAuthPolicyValue::parse))
}

/// Batch lookup over a role id list — missing roles read as unset (`None`).
/// Used by `get_conversation_detail` to pull every participant's policy in
/// one query.
pub fn find_by_roles(
    conn: &Connection,
    role_ids: &[String],
) -> Result<Vec<(String, Option<AgentAuthPolicyValue>)>, StoreError> {
    if role_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = role_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT role_id, policy FROM agent_auth_policies WHERE role_id IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql)?;
    let params = rusqlite::params_from_iter(role_ids.iter());
    let stored: Vec<(String, String)> = stmt
        .query_map(params, |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(role_ids
        .iter()
        .map(|id| {
            let policy = stored
                .iter()
                .find(|(rid, _)| rid == id)
                .and_then(|(_, p)| AgentAuthPolicyValue::parse(p));
            (id.clone(), policy)
        })
        .collect())
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

    fn seed_role(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO roles (id, name, created_at, updated_at) VALUES (?1, ?2, 1, 1)",
            params![id, format!("Role {id}")],
        )
        .unwrap();
    }

    #[test]
    fn upsert_overwrites_and_reads_back() {
        let conn = db();
        seed_role(&conn, "role_1");
        assert_eq!(find_by_role(&conn, "role_1").unwrap(), None);
        upsert(&conn, "role_1", AgentAuthPolicyValue::Authorize).unwrap();
        assert_eq!(
            find_by_role(&conn, "role_1").unwrap(),
            Some(AgentAuthPolicyValue::Authorize)
        );
        upsert(&conn, "role_1", AgentAuthPolicyValue::Deny).unwrap();
        assert_eq!(
            find_by_role(&conn, "role_1").unwrap(),
            Some(AgentAuthPolicyValue::Deny)
        );
    }

    #[test]
    fn unset_removes_row() {
        let conn = db();
        seed_role(&conn, "role_1");
        upsert(&conn, "role_1", AgentAuthPolicyValue::Authorize).unwrap();
        unset(&conn, "role_1").unwrap();
        assert_eq!(find_by_role(&conn, "role_1").unwrap(), None);
        // 幂等：对无行角色 unset 不报错
        unset(&conn, "role_404").unwrap();
    }

    #[test]
    fn upsert_missing_role_is_not_found() {
        let conn = db();
        assert!(matches!(
            upsert(&conn, "ghost", AgentAuthPolicyValue::Authorize),
            Err(StoreError::NotFound { entity: "role", .. })
        ));
    }

    #[test]
    fn batch_lookup_missing_roles_default_unset() {
        let conn = db();
        seed_role(&conn, "role_1");
        seed_role(&conn, "role_2");
        upsert(&conn, "role_1", AgentAuthPolicyValue::Deny).unwrap();
        let got =
            find_by_roles(&conn, &["role_1".into(), "role_2".into(), "role_x".into()]).unwrap();
        assert_eq!(
            got,
            vec![
                ("role_1".into(), Some(AgentAuthPolicyValue::Deny)),
                ("role_2".into(), None),
                ("role_x".into(), None),
            ]
        );
        assert!(find_by_roles(&conn, &[]).unwrap().is_empty());
    }
}
