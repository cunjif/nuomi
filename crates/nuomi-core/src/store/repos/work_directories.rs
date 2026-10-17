//! Work-directory repository (`work_directories` table, migration 0036).
//!
//! CRUD for the workspace→directory two-level model (ADR-0021). A directory
//! path is owned by at most one workspace globally: `add` moves a path in
//! from any other workspace within one transaction — the table's UNIQUE
//! constraint only covers per-workspace uniqueness. Stale directories (path
//! missing on disk) keep their rows: availability is a derived flag computed
//! from an OS existence check, never a DELETE.

use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::store::StoreError;

/// A work-directory row (mirrors the `work_directories` table).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkDirectoryEntry {
    pub id: String,
    pub workspace_id: String,
    pub abs_path: String,
    pub display_name: String,
    pub mounted: bool,
    pub added_at: i64,
}

/// A directory row with its derived availability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkDirectoryStatus {
    pub directory: WorkDirectoryEntry,
    /// The path currently exists and is a directory. `false` marks a stale
    /// directory — the row is kept and the UI renders it disabled.
    pub available: bool,
}

const ENTITY: &str = "work_directory";

/// Adds `abs_path` to a workspace (uuid-v7 id, mounted, now timestamp).
///
/// Global unique ownership (ADR-0021 decision 2): when the path currently
/// belongs to another workspace, it is removed there in the same transaction.
/// Returns `AlreadyExists` when the path is already registered for THIS
/// workspace, and `NotFound` when the workspace itself does not exist.
pub fn add(
    conn: &Connection,
    workspace_id: &str,
    abs_path: &str,
    display_name: &str,
) -> Result<WorkDirectoryEntry, StoreError> {
    let tx = conn.unchecked_transaction()?;
    // Existence pre-check keeps the FK failure from surfacing as a
    // ConstraintViolation (which would read as AlreadyExists).
    let ws_exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM workspaces WHERE id = ?1)",
        params![workspace_id],
        |r| r.get(0),
    )?;
    if !ws_exists {
        return Err(StoreError::NotFound {
            entity: "workspace",
            id: workspace_id.to_string(),
        });
    }
    tx.execute(
        "DELETE FROM work_directories WHERE abs_path = ?1 AND workspace_id <> ?2",
        params![abs_path, workspace_id],
    )?;
    let entry = WorkDirectoryEntry {
        id: Uuid::now_v7().to_string(),
        workspace_id: workspace_id.to_string(),
        abs_path: abs_path.to_string(),
        display_name: display_name.to_string(),
        mounted: true,
        added_at: crate::domain::now_ms(),
    };
    match tx.execute(
        "INSERT INTO work_directories (id, workspace_id, abs_path, display_name, mounted, added_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            entry.id,
            entry.workspace_id,
            entry.abs_path,
            entry.display_name,
            entry.mounted as i64,
            entry.added_at
        ],
    ) {
        Ok(_) => {}
        Err(rusqlite::Error::SqliteFailure(err, _))
            if err.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            return Err(StoreError::AlreadyExists {
                entity: ENTITY,
                value: abs_path.to_string(),
            });
        }
        Err(e) => return Err(StoreError::Sqlite(e)),
    }
    tx.commit()?;
    Ok(entry)
}

/// Lists a workspace's directories ordered by `added_at ASC` (id as the
/// deterministic tiebreaker).
pub fn list_by_workspace(
    conn: &Connection,
    workspace_id: &str,
) -> Result<Vec<WorkDirectoryEntry>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, abs_path, display_name, mounted, added_at
         FROM work_directories WHERE workspace_id = ?1
         ORDER BY added_at ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![workspace_id], row_to_entry)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Lists a workspace's directories with the derived `available` flag
/// (OS existence check). Stale directories keep their rows.
pub fn list_by_workspace_with_availability(
    conn: &Connection,
    workspace_id: &str,
) -> Result<Vec<WorkDirectoryStatus>, StoreError> {
    Ok(list_by_workspace(conn, workspace_id)?
        .into_iter()
        .map(|directory| WorkDirectoryStatus {
            available: std::path::Path::new(&directory.abs_path).is_dir(),
            directory,
        })
        .collect())
}

/// Finds a directory by id.
pub fn find_by_id(conn: &Connection, id: &str) -> Result<Option<WorkDirectoryEntry>, StoreError> {
    conn.query_row(
        "SELECT id, workspace_id, abs_path, display_name, mounted, added_at
         FROM work_directories WHERE id = ?1",
        params![id],
        row_to_entry,
    )
    .optional()
    .map_err(StoreError::Sqlite)
}

/// Finds the (globally unique) owning row for an absolute path.
pub fn find_by_path(
    conn: &Connection,
    abs_path: &str,
) -> Result<Option<WorkDirectoryEntry>, StoreError> {
    conn.query_row(
        "SELECT id, workspace_id, abs_path, display_name, mounted, added_at
         FROM work_directories WHERE abs_path = ?1",
        params![abs_path],
        row_to_entry,
    )
    .optional()
    .map_err(StoreError::Sqlite)
}

/// Sets the mounted flag (mount / unmount). Returns `NotFound` when the id
/// matches no row.
pub fn set_mounted(conn: &Connection, id: &str, mounted: bool) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE work_directories SET mounted = ?2 WHERE id = ?1",
        params![id, mounted as i64],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: ENTITY,
            id: id.to_string(),
        });
    }
    Ok(())
}

/// Removes a directory row by id (explicit user action only — stale
/// directories are marked via `available`, never deleted here). Does not
/// touch the filesystem.
pub fn remove(conn: &Connection, id: &str) -> Result<(), StoreError> {
    let n = conn.execute("DELETE FROM work_directories WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: ENTITY,
            id: id.to_string(),
        });
    }
    Ok(())
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkDirectoryEntry> {
    let mounted: i64 = row.get(4)?;
    Ok(WorkDirectoryEntry {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        abs_path: row.get(2)?,
        display_name: row.get(3)?,
        mounted: mounted != 0,
        added_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;
    use crate::store::repos::workspaces::{self, WorkspaceEntry};

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    fn seed_workspace(conn: &Connection, id: &str) {
        workspaces::insert(
            conn,
            &WorkspaceEntry {
                id: id.into(),
                root_path: format!("C:\\seed\\{id}"),
                color_tag: "paper-yellow".into(),
                created_at: 1,
                is_active: false,
                is_pinned: false,
            },
        )
        .unwrap();
    }

    fn is_uuid(s: &str) -> bool {
        uuid::Uuid::parse_str(s).is_ok()
    }

    #[test]
    fn add_lists_roundtrip_with_uuid_and_mount() {
        let conn = db();
        seed_workspace(&conn, "w1");
        let a = add(&conn, "w1", "C:\\proj\\alpha", "alpha").unwrap();
        let b = add(&conn, "w1", "C:\\proj\\beta", "beta").unwrap();
        assert!(is_uuid(&a.id));
        assert!(a.mounted);
        assert!(a.added_at > 0);
        let all = list_by_workspace(&conn, "w1").unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, a.id, "added_at ASC ordering");
        assert_eq!(all[1].id, b.id);
        assert_eq!(all[0].display_name, "alpha");
        assert_eq!(
            find_by_id(&conn, &b.id).unwrap().unwrap().abs_path,
            "C:\\proj\\beta"
        );
    }

    #[test]
    fn duplicate_path_within_workspace_is_already_exists() {
        let conn = db();
        seed_workspace(&conn, "w1");
        add(&conn, "w1", "C:\\proj\\alpha", "alpha").unwrap();
        let err = add(&conn, "w1", "C:\\proj\\alpha", "alpha").unwrap_err();
        assert!(matches!(
            err,
            StoreError::AlreadyExists {
                entity: "work_directory",
                ..
            }
        ));
        // 原行保留，未产生第二行
        assert_eq!(list_by_workspace(&conn, "w1").unwrap().len(), 1);
    }

    #[test]
    fn add_moves_global_ownership_across_workspaces() {
        let conn = db();
        seed_workspace(&conn, "w1");
        seed_workspace(&conn, "w2");
        seed_workspace(&conn, "w3");
        let first = add(&conn, "w1", "C:\\proj\\alpha", "alpha").unwrap();
        // 移交 w2：w1 失去该目录，全局仅剩一行
        let moved = add(&conn, "w2", "C:\\proj\\alpha", "alpha").unwrap();
        assert_ne!(moved.id, first.id, "moving creates a fresh row");
        assert!(list_by_workspace(&conn, "w1").unwrap().is_empty());
        assert_eq!(list_by_workspace(&conn, "w2").unwrap().len(), 1);
        let owner = find_by_path(&conn, "C:\\proj\\alpha").unwrap().unwrap();
        assert_eq!(owner.workspace_id, "w2");
        // 再移交 w3：w2 同样失去
        add(&conn, "w3", "C:\\proj\\alpha", "alpha").unwrap();
        assert!(list_by_workspace(&conn, "w2").unwrap().is_empty());
        assert_eq!(
            find_by_path(&conn, "C:\\proj\\alpha")
                .unwrap()
                .unwrap()
                .workspace_id,
            "w3"
        );
        // 失败的 add 不破坏现状：路径已在 w3 → AlreadyExists，行数不变
        assert!(matches!(
            add(&conn, "w3", "C:\\proj\\alpha", "alpha"),
            Err(StoreError::AlreadyExists { .. })
        ));
        assert_eq!(list_by_workspace(&conn, "w3").unwrap().len(), 1);
    }

    #[test]
    fn add_to_missing_workspace_is_not_found() {
        let conn = db();
        assert!(matches!(
            add(&conn, "ghost", "C:\\proj\\alpha", "alpha"),
            Err(StoreError::NotFound {
                entity: "workspace",
                ..
            })
        ));
    }

    #[test]
    fn set_mounted_flips_and_missing_is_not_found() {
        let conn = db();
        seed_workspace(&conn, "w1");
        let dir = add(&conn, "w1", "C:\\proj\\alpha", "alpha").unwrap();
        set_mounted(&conn, &dir.id, false).unwrap();
        assert!(!find_by_id(&conn, &dir.id).unwrap().unwrap().mounted);
        set_mounted(&conn, &dir.id, true).unwrap();
        assert!(find_by_id(&conn, &dir.id).unwrap().unwrap().mounted);
        assert!(matches!(
            set_mounted(&conn, "nope", true),
            Err(StoreError::NotFound {
                entity: "work_directory",
                ..
            })
        ));
    }

    #[test]
    fn remove_deletes_and_missing_is_not_found() {
        let conn = db();
        seed_workspace(&conn, "w1");
        let dir = add(&conn, "w1", "C:\\proj\\alpha", "alpha").unwrap();
        remove(&conn, &dir.id).unwrap();
        assert!(list_by_workspace(&conn, "w1").unwrap().is_empty());
        assert!(matches!(
            remove(&conn, &dir.id),
            Err(StoreError::NotFound {
                entity: "work_directory",
                ..
            })
        ));
    }

    #[test]
    fn stale_directory_keeps_row_and_derives_availability() {
        let conn = db();
        seed_workspace(&conn, "w1");
        let live = tempfile::tempdir().unwrap();
        let live_path = live.path().to_string_lossy().to_string();
        let stale_path = live
            .path()
            .join("never-created")
            .to_string_lossy()
            .to_string();
        add(&conn, "w1", &live_path, "live").unwrap();
        add(&conn, "w1", &stale_path, "stale").unwrap();

        let statuses = list_by_workspace_with_availability(&conn, "w1").unwrap();
        assert_eq!(statuses.len(), 2, "失效目录不删记录");
        let by_name = |n: &str| {
            statuses
                .iter()
                .find(|s| s.directory.display_name == n)
                .unwrap()
        };
        assert!(by_name("live").available);
        assert!(!by_name("stale").available);
        // 纯 DB 读路径不带派生态
        assert_eq!(list_by_workspace(&conn, "w1").unwrap().len(), 2);
    }

    #[test]
    fn workspace_removal_cascades_directories() {
        let conn = db();
        seed_workspace(&conn, "w1");
        add(&conn, "w1", "C:\\proj\\alpha", "alpha").unwrap();
        workspaces::remove(&conn, "w1").unwrap();
        assert!(list_by_workspace(&conn, "w1").unwrap().is_empty());
        assert!(find_by_path(&conn, "C:\\proj\\alpha").unwrap().is_none());
    }
}
