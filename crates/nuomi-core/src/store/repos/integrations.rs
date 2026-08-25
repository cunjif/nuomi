//! Integration registry (SPEC bots-telemetry-m1 D2): CRUD over the
//! `integrations` table introduced by migration 0004.

use rusqlite::{params, Connection, OptionalExtension};

use crate::domain::{Integration, IntegrationKind};
use crate::store::StoreError;

pub fn insert(conn: &Connection, i: &Integration) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO integrations
         (id, name, kind, config_json, events, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            i.id,
            i.name,
            i.kind.as_str(),
            serde_json::to_string(&i.config)?,
            serde_json::to_string(&i.events)?,
            i.enabled as i64,
            i.created_at,
            i.updated_at
        ],
    )?;
    Ok(())
}

pub fn get(conn: &Connection, id: &str) -> Result<Integration, StoreError> {
    conn.query_row(
        "SELECT id, name, kind, config_json, events, enabled, created_at, updated_at
         FROM integrations WHERE id = ?1",
        params![id],
        row_to_integration,
    )
    .optional()?
    .ok_or_else(|| StoreError::NotFound {
        entity: "integration",
        id: id.to_string(),
    })
}

/// Full-row update of mutable fields; `id` and `created_at` are preserved.
pub fn update(conn: &Connection, i: &Integration) -> Result<(), StoreError> {
    let n = conn.execute(
        "UPDATE integrations SET name = ?2, kind = ?3, config_json = ?4, events = ?5,
         enabled = ?6, updated_at = ?7
         WHERE id = ?1",
        params![
            i.id,
            i.name,
            i.kind.as_str(),
            serde_json::to_string(&i.config)?,
            serde_json::to_string(&i.events)?,
            i.enabled as i64,
            i.updated_at
        ],
    )?;
    if n == 0 {
        return Err(StoreError::NotFound {
            entity: "integration",
            id: i.id.clone(),
        });
    }
    Ok(())
}

pub fn list(conn: &Connection) -> Result<Vec<Integration>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, name, kind, config_json, events, enabled, created_at, updated_at
         FROM integrations ORDER BY created_at ASC",
    )?;
    let rows = stmt.query_map([], row_to_integration)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Returns whether a row was actually removed.
pub fn delete(conn: &Connection, id: &str) -> Result<bool, StoreError> {
    let n = conn.execute("DELETE FROM integrations WHERE id = ?1", params![id])?;
    Ok(n > 0)
}

pub fn count(conn: &Connection) -> Result<i64, StoreError> {
    Ok(conn.query_row("SELECT count(*) FROM integrations", [], |row| row.get(0))?)
}

// ---------------------------------------------------------------- helpers

fn parse_kind(col: usize, raw: &str) -> rusqlite::Result<IntegrationKind> {
    IntegrationKind::parse(raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            col,
            rusqlite::types::Type::Text,
            format!("unknown integration kind: {raw}").into(),
        )
    })
}

fn row_to_integration(row: &rusqlite::Row<'_>) -> rusqlite::Result<Integration> {
    let kind: String = row.get(2)?;
    let config_json: String = row.get(3)?;
    let events: String = row.get(4)?;
    Ok(Integration {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: parse_kind(2, &kind)?,
        config: crate::store::json_col(0, &config_json)?,
        events: crate::store::json_col(0, &events)?,
        enabled: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    /// Tempdir-backed SQLite (testing.md rule); `TempDir` must outlive `conn`.
    fn db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let conn = Connection::open(dir.path().join("nuomi-test.db")).unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrations::run(&conn).unwrap();
        (dir, conn)
    }

    fn integration(id: &str, name: &str) -> Integration {
        Integration {
            id: id.into(),
            name: name.into(),
            kind: IntegrationKind::FeishuBot,
            config: serde_json::json!({ "webhook_url": "https://example.test/hook" }),
            events: vec!["task.status_changed".into(), "run.state_changed".into()],
            enabled: true,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn assert_same(a: &Integration, b: &Integration) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.name, b.name);
        assert_eq!(a.kind, b.kind);
        assert_eq!(a.config, b.config);
        assert_eq!(a.events, b.events);
        assert_eq!(a.enabled, b.enabled);
        assert_eq!(a.created_at, b.created_at);
        assert_eq!(a.updated_at, b.updated_at);
    }

    #[test]
    fn insert_get_roundtrip_all_fields() {
        let (_dir, conn) = db();
        let i = integration("i1", "feishu-main");
        insert(&conn, &i).unwrap();
        let got = get(&conn, "i1").unwrap();
        assert_same(&got, &i);

        // defaults survive as empty JSON containers when stored sparsely
        let sparse = Integration {
            config: serde_json::json!({}),
            events: vec![],
            ..integration("i2", "bare")
        };
        insert(&conn, &sparse).unwrap();
        let got = get(&conn, "i2").unwrap();
        assert_same(&got, &sparse);

        // unknown id → NotFound
        assert!(matches!(
            get(&conn, "ghost"),
            Err(StoreError::NotFound {
                entity: "integration",
                ..
            })
        ));
    }

    #[test]
    fn list_orders_by_created_at_asc() {
        let (_dir, conn) = db();
        for (id, at) in [("late", 30), ("early", 10), ("mid", 20)] {
            let mut i = integration(id, id);
            i.created_at = at;
            insert(&conn, &i).unwrap();
        }
        let ids: Vec<String> = list(&conn).unwrap().into_iter().map(|i| i.id).collect();
        assert_eq!(ids, vec!["early", "mid", "late"]);
        assert_eq!(count(&conn).unwrap(), 3);
    }

    #[test]
    fn update_edits_fields_and_reports_missing() {
        let (_dir, conn) = db();
        insert(&conn, &integration("i1", "old")).unwrap();

        let mut i = get(&conn, "i1").unwrap();
        i.name = "renamed".into();
        i.kind = IntegrationKind::QqWebhook;
        i.config = serde_json::json!({
            "webhook_url": "https://example.test/qq",
            "headers": { "X-Token": "t" }
        });
        i.events = vec!["approval.requested".into()];
        i.enabled = false;
        i.updated_at = 5;
        update(&conn, &i).unwrap();

        let got = get(&conn, "i1").unwrap();
        assert_same(&got, &i);
        assert_eq!(got.updated_at, 5);

        i.id = "ghost".into();
        assert!(matches!(
            update(&conn, &i),
            Err(StoreError::NotFound {
                entity: "integration",
                ..
            })
        ));
    }

    #[test]
    fn delete_reports_presence_then_absence() {
        let (_dir, conn) = db();
        insert(&conn, &integration("i1", "one")).unwrap();
        assert!(delete(&conn, "i1").unwrap());
        assert!(!delete(&conn, "i1").unwrap());
        assert_eq!(count(&conn).unwrap(), 0);
    }

    #[test]
    fn duplicate_name_insert_fails_via_unique_constraint() {
        let (_dir, conn) = db();
        insert(&conn, &integration("i1", "same-name")).unwrap();
        assert!(insert(&conn, &integration("i2", "same-name")).is_err());
    }

    #[test]
    fn all_kinds_roundtrip_losslessly() {
        let (_dir, conn) = db();
        for (n, kind) in [
            IntegrationKind::FeishuBot,
            IntegrationKind::QqWebhook,
            IntegrationKind::Telemetry,
        ]
        .into_iter()
        .enumerate()
        {
            let mut i = integration(&format!("k{n}"), &format!("int-{n}"));
            i.kind = kind;
            insert(&conn, &i).unwrap();
            assert_eq!(get(&conn, &format!("k{n}")).unwrap().kind, kind);
        }
        // as_str / parse mirror the CHECK constraint vocabulary
        for kind in [
            IntegrationKind::FeishuBot,
            IntegrationKind::QqWebhook,
            IntegrationKind::Telemetry,
        ] {
            assert_eq!(IntegrationKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(IntegrationKind::parse("exploded"), None);
    }

    #[test]
    fn rejects_invalid_kind_via_check_constraint() {
        let (_dir, conn) = db();
        assert!(conn
            .execute(
                "INSERT INTO integrations (id, name, kind, created_at, updated_at)
                 VALUES ('bad', 'x', 'exploded', 1, 1)",
                [],
            )
            .is_err());
    }
}
