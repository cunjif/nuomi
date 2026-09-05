//! CDC change log repository: tail reads of trigger-captured table changes,
//! intended to back future push-based UI sync (replacing polling).

use rusqlite::{params, Connection};

use crate::store::StoreError;

/// The captured mutation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeOp {
    Insert,
    Update,
    Delete,
}

impl ChangeOp {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeOp::Insert => "insert",
            ChangeOp::Update => "update",
            ChangeOp::Delete => "delete",
        }
    }

    fn from_db(raw: &str) -> rusqlite::Result<ChangeOp> {
        match raw {
            "insert" => Ok(ChangeOp::Insert),
            "update" => Ok(ChangeOp::Update),
            "delete" => Ok(ChangeOp::Delete),
            other => Err(rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::<dyn std::error::Error + Send + Sync>::from(format!(
                    "unknown change_log op: {other}"
                )),
            )),
        }
    }
}

/// One captured row mutation, ordered by the monotonically increasing `seq`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeEntry {
    pub seq: i64,
    pub table_name: String,
    pub row_id: String,
    pub op: ChangeOp,
    pub changed_at: i64,
}

/// Reads up to `limit` change entries strictly after `last_seq`, ordered by
/// ascending `seq` (monotonic cursor for incremental tail consumers).
pub fn tail_after(
    conn: &Connection,
    last_seq: i64,
    limit: u32,
) -> Result<Vec<ChangeEntry>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT seq, table_name, row_id, op, changed_at
         FROM change_log WHERE seq > ?1 ORDER BY seq ASC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![last_seq, limit], |row| {
        let op_raw: String = row.get(3)?;
        Ok(ChangeEntry {
            seq: row.get(0)?,
            table_name: row.get(1)?,
            row_id: row.get(2)?,
            op: ChangeOp::from_db(&op_raw)?,
            changed_at: row.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // 0006_change_log is applied by the migrations runner itself.
        migrations::run(&conn).unwrap();
        conn
    }

    fn insert_session(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO sessions (id, title, created_at, updated_at) VALUES (?1, '', 1, 1)",
            params![id],
        )
        .unwrap();
    }

    fn insert_team(conn: &Connection, id: &str, name: &str) {
        conn.execute(
            "INSERT INTO teams (id, name, topology, created_at, updated_at)
             VALUES (?1, ?2, 'pipeline', 1, 1)",
            params![id, name],
        )
        .unwrap();
    }

    fn insert_role(conn: &Connection, id: &str, name: &str) {
        conn.execute(
            "INSERT INTO roles (id, name, created_at, updated_at) VALUES (?1, ?2, 1, 1)",
            params![id, name],
        )
        .unwrap();
    }

    fn insert_agent_profile(conn: &Connection, id: &str, name: &str) {
        conn.execute(
            "INSERT INTO agent_profiles (id, name, flavor, command, created_at, updated_at)
             VALUES (?1, ?2, 'plain', 'echo', 1, 1)",
            params![id, name],
        )
        .unwrap();
    }

    fn insert_task(conn: &Connection, id: &str, title: &str) {
        conn.execute(
            "INSERT INTO tasks (id, title, created_at, updated_at) VALUES (?1, ?2, 1, 1)",
            params![id, title],
        )
        .unwrap();
    }

    fn insert_run(conn: &Connection, id: &str, task_id: &str) {
        conn.execute(
            "INSERT INTO runs (id, task_id, session_id, heartbeat_at, created_at, updated_at)
             VALUES (?1, ?2, 's1', 1, 1, 1)",
            params![id, task_id],
        )
        .unwrap();
    }

    #[test]
    fn all_tracked_tables_capture_inserts() {
        let conn = db();
        insert_session(&conn, "s1");
        insert_team(&conn, "t1", "team-a");
        insert_role(&conn, "r1", "role-a");
        insert_agent_profile(&conn, "ap1", "profile-a");
        insert_task(&conn, "tk1", "task-a");
        insert_run(&conn, "run1", "tk1");
        let entries = tail_after(&conn, 0, 100).unwrap();
        let tables: Vec<(&str, ChangeOp)> = entries
            .iter()
            .map(|e| (e.table_name.as_str(), e.op))
            .collect();
        assert_eq!(
            tables,
            vec![
                ("sessions", ChangeOp::Insert),
                ("teams", ChangeOp::Insert),
                ("roles", ChangeOp::Insert),
                ("agent_profiles", ChangeOp::Insert),
                ("tasks", ChangeOp::Insert),
                ("runs", ChangeOp::Insert),
            ]
        );
        assert!(entries.iter().all(|e| e.changed_at > 0));
    }

    #[test]
    fn updates_and_deletes_are_captured() {
        let conn = db();
        insert_role(&conn, "r1", "role-a");
        conn.execute("UPDATE roles SET name = 'role-b' WHERE id = 'r1'", [])
            .unwrap();
        conn.execute("DELETE FROM roles WHERE id = 'r1'", [])
            .unwrap();
        let entries = tail_after(&conn, 0, 100).unwrap();
        let ops: Vec<ChangeOp> = entries.iter().map(|e| e.op).collect();
        assert_eq!(
            ops,
            vec![ChangeOp::Insert, ChangeOp::Update, ChangeOp::Delete]
        );
        assert_eq!(entries[1].row_id, "r1");
        assert_eq!(entries[2].table_name, "roles");
    }

    #[test]
    fn seq_is_monotonic_and_tail_after_pages_correctly() {
        let conn = db();
        for i in 0..5 {
            insert_session(&conn, &format!("s{i}"));
        }
        let page1 = tail_after(&conn, 0, 2).unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].seq, 1);
        assert_eq!(page1[1].seq, 2);
        let page2 = tail_after(&conn, page1[1].seq, 2).unwrap();
        assert_eq!(page2[0].seq, 3);
        assert_eq!(page2[1].seq, 4);
        let page3 = tail_after(&conn, page2[1].seq, 10).unwrap();
        assert_eq!(page3.len(), 1);
        assert_eq!(page3[0].seq, 5);
        // full read is strictly increasing
        let all = tail_after(&conn, 0, 100).unwrap();
        let seqs: Vec<i64> = all.iter().map(|e| e.seq).collect();
        let mut sorted = seqs.clone();
        sorted.sort_unstable();
        assert_eq!(seqs, sorted);
    }
}
