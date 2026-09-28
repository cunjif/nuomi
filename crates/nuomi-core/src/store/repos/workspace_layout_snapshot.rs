//! Workspace layout-snapshot repository (`workspace_layout_snapshot` table,
//! migration 0024).
//!
//! A single-row table (`id = 1`) that persists the current layout mode and
//! split-screen configuration for restoration on restart.

use rusqlite::{params, Connection, OptionalExtension};

use crate::store::StoreError;

/// A layout snapshot row — at most one exists (single-row table, `id = 1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutSnapshotRow {
    pub mode: LayoutMode,
    /// Two workspace ids for split mode; `None` in single/overview mode.
    pub split_workspace_ids: Option<[String; 2]>,
    pub focused_workspace_id: Option<String>,
    pub captured_at: i64,
}

/// Layout mode enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    Single,
    Split,
    Overview,
}

impl LayoutMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Split => "split",
            Self::Overview => "overview",
        }
    }

    pub fn parse_str(s: &str) -> Option<Self> {
        match s {
            "single" => Some(Self::Single),
            "split" => Some(Self::Split),
            "overview" => Some(Self::Overview),
            _ => None,
        }
    }
}

/// Returns the stored layout snapshot, if any.
pub fn get(conn: &Connection) -> Result<Option<LayoutSnapshotRow>, StoreError> {
    conn.query_row(
        "SELECT mode, split_workspace_ids, focused_workspace_id, captured_at
         FROM workspace_layout_snapshot WHERE id = 1",
        [],
        row_to_snapshot,
    )
    .optional()
    .map_err(StoreError::Sqlite)
}

/// Upserts the layout snapshot (single-row table, `INSERT OR REPLACE`).
pub fn upsert(conn: &Connection, row: &LayoutSnapshotRow) -> Result<(), StoreError> {
    let split_ids_json: Option<String> =
        match &row.split_workspace_ids {
            Some([a, b]) => Some(serde_json::to_string(&[a, b]).map_err(|e| {
                StoreError::Sqlite(rusqlite::Error::ToSqlConversionFailure(e.into()))
            })?),
            None => None,
        };
    conn.execute(
        "INSERT OR REPLACE INTO workspace_layout_snapshot
            (id, mode, split_workspace_ids, focused_workspace_id, captured_at)
         VALUES (1, ?1, ?2, ?3, ?4)",
        params![
            row.mode.as_str(),
            split_ids_json,
            row.focused_workspace_id,
            row.captured_at,
        ],
    )?;
    Ok(())
}

fn row_to_snapshot(row: &rusqlite::Row<'_>) -> rusqlite::Result<LayoutSnapshotRow> {
    let mode_str: String = row.get(0)?;
    let split_ids_json: Option<String> = row.get(1)?;
    let focused_workspace_id: Option<String> = row.get(2)?;
    let captured_at: i64 = row.get(3)?;

    let split_workspace_ids = split_ids_json.as_deref().and_then(|json| {
        let arr: Vec<String> = serde_json::from_str(json).ok()?;
        if arr.len() == 2 {
            Some([arr[0].clone(), arr[1].clone()])
        } else {
            None
        }
    });

    let mode = LayoutMode::parse_str(&mode_str).unwrap_or(LayoutMode::Single);

    Ok(LayoutSnapshotRow {
        mode,
        split_workspace_ids,
        focused_workspace_id,
        captured_at,
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

    #[test]
    fn get_returns_none_initially() {
        let conn = db();
        assert!(get(&conn).unwrap().is_none());
    }

    #[test]
    fn upsert_and_get_roundtrip() {
        let conn = db();
        let row = LayoutSnapshotRow {
            mode: LayoutMode::Split,
            split_workspace_ids: Some(["w1".into(), "w2".into()]),
            focused_workspace_id: Some("w1".into()),
            captured_at: 1000,
        };
        upsert(&conn, &row).unwrap();
        let result = get(&conn).unwrap().unwrap();
        assert_eq!(result.mode, LayoutMode::Split);
        assert_eq!(result.split_workspace_ids, Some(["w1".into(), "w2".into()]));
        assert_eq!(result.focused_workspace_id, Some("w1".into()));
        assert_eq!(result.captured_at, 1000);
    }

    #[test]
    fn upsert_overwrites_previous() {
        let conn = db();
        let row1 = LayoutSnapshotRow {
            mode: LayoutMode::Single,
            split_workspace_ids: None,
            focused_workspace_id: Some("w1".into()),
            captured_at: 100,
        };
        upsert(&conn, &row1).unwrap();
        let row2 = LayoutSnapshotRow {
            mode: LayoutMode::Overview,
            split_workspace_ids: None,
            focused_workspace_id: None,
            captured_at: 200,
        };
        upsert(&conn, &row2).unwrap();
        let result = get(&conn).unwrap().unwrap();
        assert_eq!(result.mode, LayoutMode::Overview);
        assert_eq!(result.captured_at, 200);
    }

    #[test]
    fn single_mode_without_split_ids() {
        let conn = db();
        let row = LayoutSnapshotRow {
            mode: LayoutMode::Single,
            split_workspace_ids: None,
            focused_workspace_id: Some("w1".into()),
            captured_at: 100,
        };
        upsert(&conn, &row).unwrap();
        let result = get(&conn).unwrap().unwrap();
        assert!(result.split_workspace_ids.is_none());
    }
}
