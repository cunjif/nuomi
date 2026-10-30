//! Self-eval temp-dir repository (`selfeval_temp_dirs` table, migration 0043).
//!
//! Tracks fourth-layer Ephemeral directory metadata for two-phase GC and
//! 10 GB LRU quota (ADR-0025 §4.2 Slice 2). The ephemeral manager is the
//! sole writer; reads are used by the GC scheduler and quota checker.

use rusqlite::{params, Connection, OptionalExtension};

use crate::store::StoreError;

/// Lifecycle status of a self-eval temp directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TempDirStatus {
    /// Just created, evaluation in progress.
    Created,
    /// Evaluation active (task running).
    Active,
    /// Evaluation ended; GC Phase 1 deferred (failure path, target/ retained 24 h).
    GcPending,
    /// Fully cleaned, directory removed.
    Cleaned,
}

impl TempDirStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Active => "active",
            Self::GcPending => "gc_pending",
            Self::Cleaned => "cleaned",
        }
    }

    fn from_str(s: &str) -> Self {
        match s {
            "active" => Self::Active,
            "gc_pending" => Self::GcPending,
            "cleaned" => Self::Cleaned,
            _ => Self::Created,
        }
    }
}

/// A `selfeval_temp_dirs` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfevalTempDirRecord {
    pub id: String,
    pub dir_path: String,
    pub status: TempDirStatus,
    pub size_bytes: i64,
    pub created_at: i64,
    /// unix-ms; `None` until status becomes `gc_pending`.
    pub gc_eligible_at: Option<i64>,
    /// unix-ms; `None` until status becomes `cleaned`.
    pub cleaned_at: Option<i64>,
}

/// Inserts a new temp-dir row at status `created`.
pub fn insert(conn: &Connection, rec: &SelfevalTempDirRecord) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO selfeval_temp_dirs
           (id, dir_path, status, size_bytes, created_at, gc_eligible_at, cleaned_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            rec.id,
            rec.dir_path,
            rec.status.as_str(),
            rec.size_bytes,
            rec.created_at,
            rec.gc_eligible_at,
            rec.cleaned_at,
        ],
    )?;
    Ok(())
}

/// Fetches a row by id.
pub fn get(conn: &Connection, id: &str) -> Result<Option<SelfevalTempDirRecord>, StoreError> {
    conn.query_row(
        "SELECT id, dir_path, status, size_bytes, created_at, gc_eligible_at, cleaned_at
         FROM selfeval_temp_dirs WHERE id = ?1",
        params![id],
        row_to_record,
    )
    .optional()
    .map_err(Into::into)
}

/// Lists all rows with a given status, ordered by `created_at` ascending.
pub fn list_by_status(
    conn: &Connection,
    status: TempDirStatus,
) -> Result<Vec<SelfevalTempDirRecord>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, dir_path, status, size_bytes, created_at, gc_eligible_at, cleaned_at
         FROM selfeval_temp_dirs WHERE status = ?1 ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map(params![status.as_str()], row_to_record)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Updates status and size; sets `gc_eligible_at` when transitioning to
/// `gc_pending`, or `cleaned_at` when transitioning to `cleaned`.
pub fn update_status(
    conn: &Connection,
    id: &str,
    status: TempDirStatus,
    size_bytes: i64,
    now_ms: i64,
) -> Result<(), StoreError> {
    let gc_eligible_at = if status == TempDirStatus::GcPending {
        Some(now_ms)
    } else {
        None
    };
    let cleaned_at = if status == TempDirStatus::Cleaned {
        Some(now_ms)
    } else {
        None
    };
    conn.execute(
        "UPDATE selfeval_temp_dirs
            SET status = ?2, size_bytes = ?3,
                gc_eligible_at = COALESCE(?4, gc_eligible_at),
                cleaned_at = COALESCE(?5, cleaned_at)
          WHERE id = ?1",
        params![id, status.as_str(), size_bytes, gc_eligible_at, cleaned_at],
    )?;
    Ok(())
}

/// Sum of `size_bytes` for all non-cleaned rows (the current disk footprint).
pub fn total_size(conn: &Connection) -> Result<i64, StoreError> {
    conn.query_row(
        "SELECT COALESCE(SUM(size_bytes), 0) FROM selfeval_temp_dirs
         WHERE status != 'cleaned'",
        [],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

/// Lists `gc_pending` rows whose `gc_eligible_at` is older than `cutoff_ms`,
/// ordered oldest-first — candidates for Phase 2 forced cleanup.
pub fn list_gc_overdue(
    conn: &Connection,
    cutoff_ms: i64,
) -> Result<Vec<SelfevalTempDirRecord>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, dir_path, status, size_bytes, created_at, gc_eligible_at, cleaned_at
         FROM selfeval_temp_dirs
         WHERE status = 'gc_pending' AND gc_eligible_at IS NOT NULL
           AND gc_eligible_at < ?1
         ORDER BY gc_eligible_at ASC",
    )?;
    let rows = stmt.query_map(params![cutoff_ms], row_to_record)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Deletes a row (called after the directory is physically removed).
pub fn delete(conn: &Connection, id: &str) -> Result<(), StoreError> {
    conn.execute("DELETE FROM selfeval_temp_dirs WHERE id = ?1", params![id])?;
    Ok(())
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<SelfevalTempDirRecord> {
    Ok(SelfevalTempDirRecord {
        id: row.get(0)?,
        dir_path: row.get(1)?,
        status: TempDirStatus::from_str(row.get::<_, String>(2)?.as_str()),
        size_bytes: row.get(3)?,
        created_at: row.get(4)?,
        gc_eligible_at: row.get(5)?,
        cleaned_at: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    fn fresh_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    #[test]
    fn insert_get_roundtrip() {
        let conn = fresh_db();
        let rec = SelfevalTempDirRecord {
            id: "t1".into(),
            dir_path: "/tmp/nuomi-selfeval-t1".into(),
            status: TempDirStatus::Created,
            size_bytes: 0,
            created_at: 1000,
            gc_eligible_at: None,
            cleaned_at: None,
        };
        insert(&conn, &rec).unwrap();
        let got = get(&conn, "t1").unwrap().unwrap();
        assert_eq!(got, rec);
    }

    #[test]
    fn update_status_sets_gc_eligible_at() {
        let conn = fresh_db();
        insert(
            &conn,
            &SelfevalTempDirRecord {
                id: "t1".into(),
                dir_path: "/tmp/x".into(),
                status: TempDirStatus::Created,
                size_bytes: 0,
                created_at: 1000,
                gc_eligible_at: None,
                cleaned_at: None,
            },
        )
        .unwrap();
        update_status(&conn, "t1", TempDirStatus::GcPending, 5000, 2000).unwrap();
        let got = get(&conn, "t1").unwrap().unwrap();
        assert_eq!(got.status, TempDirStatus::GcPending);
        assert_eq!(got.size_bytes, 5000);
        assert_eq!(got.gc_eligible_at, Some(2000));
        assert!(got.cleaned_at.is_none());
    }

    #[test]
    fn update_status_sets_cleaned_at() {
        let conn = fresh_db();
        insert(
            &conn,
            &SelfevalTempDirRecord {
                id: "t1".into(),
                dir_path: "/tmp/x".into(),
                status: TempDirStatus::Created,
                size_bytes: 0,
                created_at: 1000,
                gc_eligible_at: None,
                cleaned_at: None,
            },
        )
        .unwrap();
        update_status(&conn, "t1", TempDirStatus::Cleaned, 0, 3000).unwrap();
        let got = get(&conn, "t1").unwrap().unwrap();
        assert_eq!(got.status, TempDirStatus::Cleaned);
        assert_eq!(got.cleaned_at, Some(3000));
    }

    #[test]
    fn total_size_excludes_cleaned() {
        let conn = fresh_db();
        for (id, sz, status) in [
            ("a", 1000, TempDirStatus::Created),
            ("b", 2000, TempDirStatus::GcPending),
            ("c", 3000, TempDirStatus::Cleaned),
        ] {
            insert(
                &conn,
                &SelfevalTempDirRecord {
                    id: id.into(),
                    dir_path: format!("/tmp/{id}"),
                    status,
                    size_bytes: sz,
                    created_at: 1000,
                    gc_eligible_at: None,
                    cleaned_at: None,
                },
            )
            .unwrap();
        }
        assert_eq!(total_size(&conn).unwrap(), 3000);
    }

    #[test]
    fn list_gc_overdue_filters_by_cutoff() {
        let conn = fresh_db();
        for (id, gc_at) in [("old", 1000), ("new", 5000)] {
            insert(
                &conn,
                &SelfevalTempDirRecord {
                    id: id.into(),
                    dir_path: format!("/tmp/{id}"),
                    status: TempDirStatus::GcPending,
                    size_bytes: 100,
                    created_at: 500,
                    gc_eligible_at: Some(gc_at),
                    cleaned_at: None,
                },
            )
            .unwrap();
        }
        let overdue = list_gc_overdue(&conn, 3000).unwrap();
        assert_eq!(overdue.len(), 1);
        assert_eq!(overdue[0].id, "old");
    }
}
