//! SQLite persistence: repositories and migrations runner.
//!
//! Rules: all SQL lives here (parameterized only); migrations are append-only
//! numbered files executed lexicographically, tracked via `PRAGMA user_version`.

pub mod migrations;
pub mod repos;

use thiserror::Error;

/// Errors produced by the storage layer.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("migration error at {name}: {source}")]
    Migration {
        name: String,
        source: rusqlite::Error,
    },

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("not found: {entity}#{id}")]
    NotFound { entity: &'static str, id: String },
}

/// A handle to a SQLite connection guarded for blocking use inside
/// `spawn_blocking` contexts (never call from async directly).
pub struct Db(pub rusqlite::Connection);

/// Deserializes a TEXT column holding JSON, mapped to `rusqlite::Error`.
pub(crate) fn json_col<T: serde::de::DeserializeOwned>(
    col: usize,
    raw: &str,
) -> rusqlite::Result<T> {
    serde_json::from_str(raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(col, rusqlite::types::Type::Text, Box::new(e))
    })
}

impl Db {
    /// Opens (creating if needed) a database file with WAL enabled.
    pub fn open(path: &str) -> Result<Self, StoreError> {
        let conn = rusqlite::Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(Self(conn))
    }

    /// Opens an in-memory database (tests).
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = rusqlite::Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(Self(conn))
    }
}
