//! Repository for advisory team-formation proposals (migration 0039, P2-F
//! 组 2). Insert/list/resolve with the same optimistic pending-guard as the
//! approvals repo: a resolved proposal can never be flipped again.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{Proposal, ProposalStatus};
use crate::store::StoreError;

pub fn insert(conn: &Connection, p: &Proposal) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO proposals (id, kind, source, plan_json, status, workspace_id, resolved_at, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            p.id,
            p.kind,
            p.source,
            p.plan_json,
            p.status.as_str(),
            p.workspace_id,
            p.resolved_at,
            p.created_at
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Proposal, StoreError> {
    conn.query_row(
        &proposal_select("WHERE id = ?1"),
        params![id],
        row_to_proposal,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "proposal",
        id: id.to_string(),
    })
}

/// Resolves a pending proposal; guarded so a decision can never be flipped.
pub fn resolve(
    conn: &Connection,
    id: &str,
    status: ProposalStatus,
    resolved_at: i64,
) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE proposals SET status = ?2, resolved_at = ?3
         WHERE id = ?1 AND status = 'pending'",
        params![id, status.as_str(), resolved_at],
    )?;
    if n > 0 {
        return Ok(());
    }
    // Distinguish "row gone" from "already decided": a NotFound propagates,
    // an existing row means the pending guard rejected the flip.
    match get(conn, id) {
        Ok(_) => Err(StoreError::Conflict {
            entity: "proposal",
            id: id.to_string(),
            expected: ProposalStatus::Pending.as_str().to_string(),
        }),
        Err(not_found) => Err(not_found),
    }
}

pub fn list_pending(conn: &Connection) -> Result<Vec<Proposal>, StoreError> {
    let mut stmt = conn.prepare(&proposal_select(
        "WHERE status = 'pending' ORDER BY created_at ASC",
    ))?;
    let rows = stmt.query_map([], row_to_proposal)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Pending proposals of one workspace, ordered `created_at ASC`.
pub fn list_pending_by_workspace(
    conn: &Connection,
    workspace_id: &str,
) -> Result<Vec<Proposal>, StoreError> {
    let mut stmt = conn.prepare(&proposal_select(
        "WHERE workspace_id = ?1 AND status = 'pending' ORDER BY created_at ASC",
    ))?;
    let rows = stmt.query_map(params![workspace_id], row_to_proposal)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn proposal_select(where_clause: &str) -> String {
    format!(
        "SELECT id, kind, source, plan_json, status, workspace_id, resolved_at, created_at
         FROM proposals {where_clause}"
    )
}

fn row_to_proposal(row: &rusqlite::Row<'_>) -> rusqlite::Result<Proposal> {
    let status: String = row.get(4)?;
    let status = ProposalStatus::parse(&status).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            format!("unknown proposal status: {status}").into(),
        )
    })?;
    Ok(Proposal {
        id: row.get(0)?,
        kind: row.get(1)?,
        source: row.get(2)?,
        plan_json: row.get(3)?,
        status,
        workspace_id: row.get(5)?,
        resolved_at: row.get(6)?,
        created_at: row.get(7)?,
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

    fn proposal(id: &str, workspace_id: &str) -> Proposal {
        Proposal {
            id: id.into(),
            kind: "team_plan".into(),
            source: "scheduled".into(),
            plan_json: r#"{"task":"fix it","taskIds":["t1"],"topology":"pipeline"}"#.into(),
            status: ProposalStatus::Pending,
            workspace_id: workspace_id.into(),
            resolved_at: None,
            created_at: 1,
        }
    }

    #[test]
    fn insert_get_roundtrip() {
        let conn = db();
        insert(&conn, &proposal("p1", "__migrated__")).unwrap();
        let loaded = get(&conn, "p1").unwrap();
        assert_eq!(loaded.kind, "team_plan");
        assert_eq!(loaded.source, "scheduled");
        assert_eq!(loaded.status, ProposalStatus::Pending);
        assert!(loaded.plan_json.contains("pipeline"));
        assert!(get(&conn, "missing").is_err());
    }

    #[test]
    fn list_pending_filters_by_workspace_and_status() {
        let conn = db();
        insert(&conn, &proposal("p1", "ws-a")).unwrap();
        insert(&conn, &proposal("p2", "ws-b")).unwrap();
        let mut resolved = proposal("p3", "ws-a");
        resolved.status = ProposalStatus::Denied;
        insert(&conn, &resolved).unwrap();

        let all = list_pending(&conn).unwrap();
        assert_eq!(all.len(), 2);
        let ws = list_pending_by_workspace(&conn, "ws-a").unwrap();
        assert_eq!(ws.len(), 1);
        assert_eq!(ws[0].id, "p1");
    }

    #[test]
    fn resolve_is_guarded_against_flipping() {
        let conn = db();
        insert(&conn, &proposal("p1", "__migrated__")).unwrap();
        resolve(&conn, "p1", ProposalStatus::Approved, 99).unwrap();
        let loaded = get(&conn, "p1").unwrap();
        assert_eq!(loaded.status, ProposalStatus::Approved);
        assert_eq!(loaded.resolved_at, Some(99));

        // second resolution must fail (pending guard).
        let flip = resolve(&conn, "p1", ProposalStatus::Denied, 100);
        assert!(flip.is_err(), "resolved proposal cannot be flipped");
        assert_eq!(get(&conn, "p1").unwrap().status, ProposalStatus::Approved);

        // unknown id → NotFound.
        assert!(resolve(&conn, "missing", ProposalStatus::Denied, 1).is_err());
    }

    #[test]
    fn pending_list_orders_by_created_at() {
        let conn = db();
        let mut later = proposal("p2", "__migrated__");
        later.created_at = 5;
        let mut earlier = proposal("p1", "__migrated__");
        earlier.created_at = 2;
        insert(&conn, &later).unwrap();
        insert(&conn, &earlier).unwrap();
        let pending = list_pending(&conn).unwrap();
        assert_eq!(pending[0].id, "p1");
        assert_eq!(pending[1].id, "p2");
    }
}
