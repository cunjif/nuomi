//! Product registry repository (`product_registry` table, migration 0049).
//!
//! Row-level persistence for the unified product directory: every artifact
//! placed under `.nuomi/products/` is indexed here so the six modules can
//! discover and consume each other's outputs via SQL queries rather than
//! filesystem scans.

use rusqlite::{params, Connection, OptionalExtension};

use crate::services::product_dir::{ProductKind, Producer, ProducerModule};
use crate::store::StoreError;

/// A `product_registry` row.
#[derive(Debug, Clone)]
pub struct ProductRecord {
    pub id: i64,
    pub kind: ProductKind,
    pub producer: Producer,
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub key: String,
    pub abs_path: String,
    pub size_bytes: i64,
    pub sha256: Option<String>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub metadata: Option<serde_json::Value>,
}

/// Filter for `list`.
#[derive(Debug, Clone, Default)]
pub struct ProductFilter {
    pub kind: Option<ProductKind>,
    pub producer_module: Option<ProducerModule>,
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
}



/// Inserts a product record. Returns the auto-incremented row id.
pub fn insert(conn: &Connection, record: &InsertParams) -> Result<i64, StoreError> {
    conn.query_row(
        "INSERT INTO product_registry
           (kind, producer, workspace_id, session_id, run_id, key,
            abs_path, size_bytes, sha256, created_at, expires_at, metadata)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         RETURNING id",
        params![
            record.kind.as_str(),
            record.producer.as_string(),
            record.workspace_id,
            record.session_id,
            record.run_id,
            record.key,
            record.abs_path,
            record.size_bytes,
            record.sha256,
            record.created_at,
            record.expires_at,
            record.metadata.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()),
        ],
        |row| row.get(0),
    )
    .map_err(StoreError::from)
}

/// Parameters for inserting a product record.
#[derive(Debug, Clone)]
pub struct InsertParams {
    pub kind: ProductKind,
    pub producer: Producer,
    pub workspace_id: Option<String>,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub key: String,
    pub abs_path: String,
    pub size_bytes: i64,
    pub sha256: Option<String>,
    pub created_at: i64,
    pub expires_at: Option<i64>,
    pub metadata: Option<serde_json::Value>,
}

/// Fetches a product by row id.
pub fn get(conn: &Connection, id: i64) -> Result<Option<ProductRecord>, StoreError> {
    conn.query_row(
        "SELECT id, kind, producer, workspace_id, session_id, run_id, key,
                abs_path, size_bytes, sha256, created_at, expires_at, metadata
         FROM product_registry WHERE id = ?1",
        params![id],
        row_to_record,
    )
    .optional()
    .map_err(StoreError::from)
}

/// Lists products matching the filter, most recent first.
pub fn list(conn: &Connection, filter: &ProductFilter) -> Result<Vec<ProductRecord>, StoreError> {
    let mut sql = String::from(
        "SELECT id, kind, producer, workspace_id, session_id, run_id, key,
                abs_path, size_bytes, sha256, created_at, expires_at, metadata
         FROM product_registry WHERE 1=1",
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(kind) = filter.kind {
        sql.push_str(" AND kind = ?");
        args.push(Box::new(kind.as_str().to_string()));
    }
    if let Some(module) = filter.producer_module {
        sql.push_str(" AND producer LIKE ?");
        args.push(Box::new(format!("{}:%", module.as_str())));
    }
    if let Some(ws) = &filter.workspace_id {
        sql.push_str(" AND workspace_id = ?");
        args.push(Box::new(ws.clone()));
    }
    if let Some(session) = &filter.session_id {
        sql.push_str(" AND session_id = ?");
        args.push(Box::new(session.clone()));
    }
    if let Some(run) = &filter.run_id {
        sql.push_str(" AND run_id = ?");
        args.push(Box::new(run.clone()));
    }
    sql.push_str(" ORDER BY created_at DESC");

    let arg_refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(StoreError::from)?;
    let rows = stmt
        .query_map(arg_refs.as_slice(), row_to_record)
        .map_err(StoreError::from)?;
    let mut records = Vec::new();
    for row in rows {
        records.push(row.map_err(StoreError::from)?);
    }
    Ok(records)
}

/// Deletes expired products (where `expires_at` is not NULL and < `now_ms`).
/// Returns the number of deleted rows.
pub fn gc_expired(conn: &Connection, now_ms: i64) -> Result<usize, StoreError> {
    conn.execute(
        "DELETE FROM product_registry WHERE expires_at IS NOT NULL AND expires_at < ?1",
        params![now_ms],
    )
    .map_err(StoreError::from)
}

/// Deletes a product by row id.
pub fn delete(conn: &Connection, id: i64) -> Result<(), StoreError> {
    conn.execute(
        "DELETE FROM product_registry WHERE id = ?1",
        params![id],
    )
    .map_err(StoreError::from)?;
    Ok(())
}

/// Finds a product by kind + producer + key (unique business key).
pub fn find_by_key(
    conn: &Connection,
    kind: ProductKind,
    producer: &Producer,
    key: &str,
) -> Result<Option<ProductRecord>, StoreError> {
    conn.query_row(
        "SELECT id, kind, producer, workspace_id, session_id, run_id, key,
                abs_path, size_bytes, sha256, created_at, expires_at, metadata
         FROM product_registry
         WHERE kind = ?1 AND producer = ?2 AND key = ?3
         ORDER BY created_at DESC LIMIT 1",
        params![kind.as_str(), producer.as_string(), key],
        row_to_record,
    )
    .optional()
    .map_err(StoreError::from)
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProductRecord> {
    let kind_str: String = row.get(1)?;
    let producer_str: String = row.get(2)?;
    let metadata_str: Option<String> = row.get(12)?;

    let kind = ProductKind::parse(&kind_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown product kind: {kind_str}"),
            )),
        )
    })?;
    let producer = Producer::parse(&producer_str).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unknown producer: {producer_str}"),
            )),
        )
    })?;

    Ok(ProductRecord {
        id: row.get(0)?,
        kind,
        producer,
        workspace_id: row.get(3)?,
        session_id: row.get(4)?,
        run_id: row.get(5)?,
        key: row.get(6)?,
        abs_path: row.get(7)?,
        size_bytes: row.get(8)?,
        sha256: row.get(9)?,
        created_at: row.get(10)?,
        expires_at: row.get(11)?,
        metadata: metadata_str
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;

    fn open_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    #[test]
    fn insert_and_get() {
        let conn = open_db();
        let params = InsertParams {
            kind: ProductKind::ToolResult,
            producer: Producer::new(ProducerModule::Loop, "run-1"),
            workspace_id: Some("ws-1".into()),
            session_id: Some("s-1".into()),
            run_id: Some("run-1".into()),
            key: "call-42".into(),
            abs_path: "/tmp/nuomi/products/tool_result/loop/run-1/call-42".into(),
            size_bytes: 1024,
            sha256: Some("abc123".into()),
            created_at: 1000,
            expires_at: Some(1000 + 7 * 24 * 3600 * 1000),
            metadata: Some(serde_json::json!({"mime": "text/plain"})),
        };
        let id = insert(&conn, &params).unwrap();
        assert!(id > 0);

        let record = get(&conn, id).unwrap().unwrap();
        assert_eq!(record.kind, ProductKind::ToolResult);
        assert_eq!(record.producer.module, ProducerModule::Loop);
        assert_eq!(record.key, "call-42");
        assert_eq!(record.size_bytes, 1024);
    }

    #[test]
    fn list_by_kind() {
        let conn = open_db();
        for i in 0..3 {
            insert(&conn, &InsertParams {
                kind: ProductKind::ToolResult,
                producer: Producer::new(ProducerModule::Loop, format!("run-{i}")),
                workspace_id: None,
                session_id: None,
                run_id: None,
                key: format!("call-{i}"),
                abs_path: format!("/tmp/call-{i}"),
                size_bytes: 100,
                sha256: None,
                created_at: 1000 + i,
                expires_at: None,
                metadata: None,
            })
            .unwrap();
        }
        insert(&conn, &InsertParams {
            kind: ProductKind::SandboxOutput,
            producer: Producer::new(ProducerModule::Sandbox, "task-1"),
            workspace_id: None,
            session_id: None,
            run_id: None,
            key: "stdout.log".into(),
            abs_path: "/tmp/stdout.log".into(),
            size_bytes: 200,
            sha256: None,
            created_at: 2000,
            expires_at: None,
            metadata: None,
        })
        .unwrap();

        let filter = ProductFilter {
            kind: Some(ProductKind::ToolResult),
            ..Default::default()
        };
        let results = list(&conn, &filter).unwrap();
        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.kind == ProductKind::ToolResult));
    }

    #[test]
    fn gc_expired_deletes_past_expiry() {
        let conn = open_db();
        insert(&conn, &InsertParams {
            kind: ProductKind::SearchResult,
            producer: Producer::new(ProducerModule::CrossWorkspace, "q-1"),
            workspace_id: None,
            session_id: None,
            run_id: None,
            key: "hash-1".into(),
            abs_path: "/tmp/search-1".into(),
            size_bytes: 50,
            sha256: None,
            created_at: 1000,
            expires_at: Some(500), // already expired
            metadata: None,
        })
        .unwrap();
        insert(&conn, &InsertParams {
            kind: ProductKind::SearchResult,
            producer: Producer::new(ProducerModule::CrossWorkspace, "q-2"),
            workspace_id: None,
            session_id: None,
            run_id: None,
            key: "hash-2".into(),
            abs_path: "/tmp/search-2".into(),
            size_bytes: 50,
            sha256: None,
            created_at: 1000,
            expires_at: Some(5000), // not yet expired
            metadata: None,
        })
        .unwrap();

        let deleted = gc_expired(&conn, 1000).unwrap();
        assert_eq!(deleted, 1);

        let remaining = list(&conn, &ProductFilter::default()).unwrap();
        assert_eq!(remaining.len(), 1);
    }

    #[test]
    fn find_by_key_test() {
        let conn = open_db();
        insert(&conn, &InsertParams {
            kind: ProductKind::TerminalLog,
            producer: Producer::new(ProducerModule::Terminal, "term-1"),
            workspace_id: Some("ws-1".into()),
            session_id: None,
            run_id: None,
            key: "output.jsonl".into(),
            abs_path: "/tmp/term-1/output.jsonl".into(),
            size_bytes: 500,
            sha256: None,
            created_at: 1000,
            expires_at: None,
            metadata: None,
        })
        .unwrap();

        let found = find_by_key(
            &conn,
            ProductKind::TerminalLog,
            &Producer::new(ProducerModule::Terminal, "term-1"),
            "output.jsonl",
        )
        .unwrap()
        .unwrap();
        assert_eq!(found.abs_path, "/tmp/term-1/output.jsonl");
    }
}
