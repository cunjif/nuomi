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

    #[error("conflict on {entity}#{id}: expected status '{expected}'")]
    Conflict {
        entity: &'static str,
        id: String,
        expected: String,
    },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
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
    ///
    /// Parent directories are created too: SQLite never creates them, and a
    /// fresh machine has no `app_data_dir` yet (e.g. Tauri's roaming dir).
    pub fn open(path: &str) -> Result<Self, StoreError> {
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        // Two levels below the temp root: neither exists yet.
        let db_path = dir.path().join("deep/nested/nuomi.db");
        let path = db_path.to_string_lossy().to_string();

        let db = Db::open(&path).expect("open should create parents");
        migrations::run(&db.0).unwrap();
        assert!(db_path.exists());
    }

    #[test]
    fn open_rejects_uncreatable_parent() {
        // A path under a "file" that already exists cannot become a directory.
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        let path = blocker.join("nuomi.db").to_string_lossy().to_string();

        assert!(Db::open(&path).is_err());
    }
}
