//! Fourth-layer Ephemeral directory manager (ADR-0025 §4.2 Slice 2).
//!
//! Manages `nuomi-selfeval-{id}` directories under an application-level
//! Ephemeral root (`<ephemeral_root>/nuomi-selfeval-{id}`). Provides:
//!
//! - **Creation** with 10 GB hard-quota LRU eviction (oldest `gc_pending`
//!   dirs are physically removed before a new dir is created).
//! - **Phase 1 GC** (on assessment end): clears the source clone and
//!   `node_modules/.cache`; on success also clears `target/`; on failure
//!   retains `target/` for 24 h (configurable TTL) for incremental recompile.
//! - **Phase 2 GC** (on evolution success): force-removes all remaining
//!   intermediates; the directory row is marked `cleaned` then deleted.
//!
//! The manager is the sole writer of `selfeval_temp_dirs` rows. All file
//! operations are best-effort: a failed `remove_dir_all` is logged but does
//! not prevent the DB row update (the next GC sweep will retry).

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::domain::new_id;
use crate::store::repos::selfeval_temp_dirs;
use crate::store::StoreError;

/// Default disk quota for all non-cleaned temp dirs (10 GB).
pub const DEFAULT_QUOTA_BYTES: i64 = 10 * 1024 * 1024 * 1024;

/// Default TTL for `target/` retention on failure (24 h in ms).
pub const DEFAULT_TARGET_RETENTION_MS: i64 = 24 * 60 * 60 * 1000;

/// Errors from the ephemeral manager.
#[derive(Debug, thiserror::Error)]
pub enum EphemeralError {
    #[error("disk quota exceeded: current {current} + needed {needed} > quota {quota}")]
    QuotaExceeded {
        current: i64,
        needed: i64,
        quota: i64,
    },
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Configuration for the ephemeral manager.
#[derive(Debug, Clone)]
pub struct EphemeralConfig {
    /// Absolute path to the Ephemeral root (e.g. `<app_data_dir>/ephemeral`).
    pub root: PathBuf,
    /// Hard disk quota for all non-cleaned dirs.
    pub quota_bytes: i64,
    /// How long to retain `target/` after a failed assessment.
    pub target_retention_ms: i64,
}

impl Default for EphemeralConfig {
    fn default() -> Self {
        Self {
            root: PathBuf::from("."),
            quota_bytes: DEFAULT_QUOTA_BYTES,
            target_retention_ms: DEFAULT_TARGET_RETENTION_MS,
        }
    }
}

/// A handle to a freshly created temp dir.
#[derive(Debug, Clone)]
pub struct TempDirHandle {
    pub id: String,
    pub path: PathBuf,
}

/// The ephemeral directory manager.
pub struct EphemeralManager {
    cfg: EphemeralConfig,
}

impl EphemeralManager {
    pub fn new(cfg: EphemeralConfig) -> Self {
        Self { cfg }
    }

    /// Returns the expected directory path for a given id.
    pub fn dir_path(&self, id: &str) -> PathBuf {
        self.cfg.root.join(format!("nuomi-selfeval-{id}"))
    }

    /// Creates a new `nuomi-selfeval-{id}` directory, evicting LRU
    /// `gc_pending` dirs first if the quota would be exceeded.
    ///
    /// Inserts a `created` row in `selfeval_temp_dirs`. The caller is
    /// responsible for transitioning the row to `active` / `gc_pending` /
    /// `cleaned` via the GC methods below.
    pub fn create(&self, conn: &Connection, now_ms: i64) -> Result<TempDirHandle, EphemeralError> {
        self.evict_if_needed(conn, 0)?;

        let id = new_id();
        let path = self.dir_path(&id);
        std::fs::create_dir_all(&path)?;

        let rec = selfeval_temp_dirs::SelfevalTempDirRecord {
            id: id.clone(),
            dir_path: path.to_string_lossy().into_owned(),
            status: selfeval_temp_dirs::TempDirStatus::Created,
            size_bytes: 0,
            created_at: now_ms,
            gc_eligible_at: None,
            cleaned_at: None,
        };
        selfeval_temp_dirs::insert(conn, &rec)?;

        Ok(TempDirHandle { id, path })
    }

    /// Phase 1 GC: called when a sandbox assessment reaches a terminal state.
    ///
    /// - `success`: clears source clone + `node_modules/.cache` + `target/`.
    /// - `!success`: clears source clone + `node_modules/.cache` only;
    ///   retains `target/` for incremental recompile; row → `gc_pending`.
    ///
    /// In both cases the row's `size_bytes` is refreshed from the filesystem.
    pub fn gc_phase1(
        &self,
        conn: &Connection,
        dir_id: &str,
        success: bool,
        now_ms: i64,
    ) -> Result<(), EphemeralError> {
        let rec = match selfeval_temp_dirs::get(conn, dir_id)? {
            Some(r) => r,
            None => return Ok(()),
        };
        let path = PathBuf::from(&rec.dir_path);

        // Always clear the source clone and node_modules cache.
        self.remove_best_effort(&path.join("node_modules").join(".cache"));
        self.remove_best_effort(&path.join("src"));

        if success {
            self.remove_best_effort(&path.join("target"));
            let size = dir_size(&path);
            selfeval_temp_dirs::update_status(
                conn,
                dir_id,
                selfeval_temp_dirs::TempDirStatus::Cleaned,
                size,
                now_ms,
            )?;
        } else {
            let size = dir_size(&path);
            selfeval_temp_dirs::update_status(
                conn,
                dir_id,
                selfeval_temp_dirs::TempDirStatus::GcPending,
                size,
                now_ms,
            )?;
        }
        Ok(())
    }

    /// Phase 2 GC: force-removes all remaining intermediates and the
    /// directory itself. Called after a successful evolution host replacement.
    pub fn gc_phase2(
        &self,
        conn: &Connection,
        dir_id: &str,
        now_ms: i64,
    ) -> Result<(), EphemeralError> {
        let rec = match selfeval_temp_dirs::get(conn, dir_id)? {
            Some(r) => r,
            None => return Ok(()),
        };
        let path = PathBuf::from(&rec.dir_path);
        self.remove_best_effort(&path);

        selfeval_temp_dirs::update_status(
            conn,
            dir_id,
            selfeval_temp_dirs::TempDirStatus::Cleaned,
            0,
            now_ms,
        )?;
        selfeval_temp_dirs::delete(conn, dir_id)?;
        Ok(())
    }

    /// Sweeps all `gc_pending` dirs whose retention TTL has expired,
    /// force-removing them. Called periodically (e.g. on app boot or
    /// before creating a new dir).
    pub fn sweep_expired(&self, conn: &Connection, now_ms: i64) -> Result<usize, EphemeralError> {
        let cutoff = now_ms - self.cfg.target_retention_ms;
        let overdue = selfeval_temp_dirs::list_gc_overdue(conn, cutoff)?;
        let mut count = 0;
        for rec in overdue {
            self.remove_best_effort(&PathBuf::from(&rec.dir_path));
            selfeval_temp_dirs::update_status(
                conn,
                &rec.id,
                selfeval_temp_dirs::TempDirStatus::Cleaned,
                0,
                now_ms,
            )?;
            selfeval_temp_dirs::delete(conn, &rec.id)?;
            count += 1;
        }
        Ok(count)
    }

    /// Checks the quota and evicts oldest `gc_pending` dirs until the
    /// projected footprint fits. `needed` is the additional bytes the
    /// caller intends to allocate.
    fn evict_if_needed(&self, conn: &Connection, needed: i64) -> Result<(), EphemeralError> {
        let current = selfeval_temp_dirs::total_size(conn)?;
        if current + needed <= self.cfg.quota_bytes {
            return Ok(());
        }
        // Evict oldest gc_pending dirs until we fit.
        let pending =
            selfeval_temp_dirs::list_by_status(conn, selfeval_temp_dirs::TempDirStatus::GcPending)?;
        let mut freed = 0i64;
        for rec in pending {
            if current - freed + needed <= self.cfg.quota_bytes {
                break;
            }
            self.remove_best_effort(&PathBuf::from(&rec.dir_path));
            selfeval_temp_dirs::delete(conn, &rec.id)?;
            freed += rec.size_bytes;
        }
        let remaining = current - freed;
        if remaining + needed > self.cfg.quota_bytes {
            return Err(EphemeralError::QuotaExceeded {
                current: remaining,
                needed,
                quota: self.cfg.quota_bytes,
            });
        }
        Ok(())
    }

    fn remove_best_effort(&self, path: &Path) {
        if path.exists() {
            if let Err(e) = std::fs::remove_dir_all(path) {
                tracing::warn!(error = %e, ?path, "ephemeral GC remove failed");
            }
        }
    }
}

/// Recursively computes the size of a directory in bytes.
fn dir_size(path: &Path) -> i64 {
    let mut total: i64 = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += dir_size(&p);
            } else if let Ok(meta) = entry.metadata() {
                total += meta.len() as i64;
            }
        }
    }
    total
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

    fn temp_root() -> PathBuf {
        let root = std::env::temp_dir().join(format!("nuomi-test-ephemeral-{}", new_id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn create_inserts_row_and_makes_dir() {
        let conn = fresh_db();
        let root = temp_root();
        let mgr = EphemeralManager::new(EphemeralConfig {
            root: root.clone(),
            ..Default::default()
        });
        let handle = mgr.create(&conn, 1000).unwrap();
        assert!(handle.path.is_dir());
        assert!(handle.path.to_string_lossy().contains("nuomi-selfeval-"));
        let rec = selfeval_temp_dirs::get(&conn, &handle.id).unwrap().unwrap();
        assert_eq!(rec.status, selfeval_temp_dirs::TempDirStatus::Created);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn gc_phase1_success_clears_and_marks_cleaned() {
        let conn = fresh_db();
        let root = temp_root();
        let mgr = EphemeralManager::new(EphemeralConfig {
            root: root.clone(),
            ..Default::default()
        });
        let handle = mgr.create(&conn, 1000).unwrap();
        // Simulate build artifacts.
        std::fs::create_dir_all(handle.path.join("target")).unwrap();
        std::fs::write(handle.path.join("target").join("obj.o"), b"x").unwrap();
        std::fs::create_dir_all(handle.path.join("src")).unwrap();
        std::fs::write(handle.path.join("src").join("main.rs"), b"fn main(){}").unwrap();

        mgr.gc_phase1(&conn, &handle.id, true, 2000).unwrap();

        assert!(!handle.path.join("target").exists());
        assert!(!handle.path.join("src").exists());
        let rec = selfeval_temp_dirs::get(&conn, &handle.id).unwrap().unwrap();
        assert_eq!(rec.status, selfeval_temp_dirs::TempDirStatus::Cleaned);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn gc_phase1_failure_retains_target() {
        let conn = fresh_db();
        let root = temp_root();
        let mgr = EphemeralManager::new(EphemeralConfig {
            root: root.clone(),
            ..Default::default()
        });
        let handle = mgr.create(&conn, 1000).unwrap();
        std::fs::create_dir_all(handle.path.join("target")).unwrap();
        std::fs::write(handle.path.join("target").join("obj.o"), b"x").unwrap();
        std::fs::create_dir_all(handle.path.join("src")).unwrap();
        std::fs::write(handle.path.join("src").join("main.rs"), b"fn main(){}").unwrap();

        mgr.gc_phase1(&conn, &handle.id, false, 2000).unwrap();

        assert!(
            handle.path.join("target").exists(),
            "target retained on failure"
        );
        assert!(!handle.path.join("src").exists());
        let rec = selfeval_temp_dirs::get(&conn, &handle.id).unwrap().unwrap();
        assert_eq!(rec.status, selfeval_temp_dirs::TempDirStatus::GcPending);
        assert_eq!(rec.gc_eligible_at, Some(2000));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn gc_phase2_removes_dir_and_deletes_row() {
        let conn = fresh_db();
        let root = temp_root();
        let mgr = EphemeralManager::new(EphemeralConfig {
            root: root.clone(),
            ..Default::default()
        });
        let handle = mgr.create(&conn, 1000).unwrap();
        std::fs::write(handle.path.join("leftover.bin"), b"x").unwrap();

        mgr.gc_phase2(&conn, &handle.id, 2000).unwrap();

        assert!(!handle.path.exists());
        assert!(selfeval_temp_dirs::get(&conn, &handle.id)
            .unwrap()
            .is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn sweep_expired_clears_overdue_gc_pending() {
        let conn = fresh_db();
        let root = temp_root();
        let mgr = EphemeralManager::new(EphemeralConfig {
            root: root.clone(),
            target_retention_ms: 1000,
            ..Default::default()
        });
        let h1 = mgr.create(&conn, 1000).unwrap();
        mgr.gc_phase1(&conn, &h1.id, false, 1000).unwrap();
        // h1 is gc_pending at t=1000; with 1000ms TTL, at t=2500 it's overdue.
        let swept = mgr.sweep_expired(&conn, 2500).unwrap();
        assert_eq!(swept, 1);
        assert!(!h1.path.exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn quota_exceeded_returns_error() {
        let conn = fresh_db();
        let root = temp_root();
        let mgr = EphemeralManager::new(EphemeralConfig {
            root: root.clone(),
            quota_bytes: 1, // 1 byte quota — impossible to fit anything
            ..Default::default()
        });
        // First create succeeds (size 0), but we simulate a large dir.
        let h1 = mgr.create(&conn, 1000).unwrap();
        selfeval_temp_dirs::update_status(
            &conn,
            &h1.id,
            selfeval_temp_dirs::TempDirStatus::Active,
            100,
            1000,
        )
        .unwrap();
        // Now quota is 1, current is 100 — second create should fail.
        let result = mgr.create(&conn, 2000);
        assert!(matches!(result, Err(EphemeralError::QuotaExceeded { .. })));
        std::fs::remove_dir_all(&root).ok();
    }
}
