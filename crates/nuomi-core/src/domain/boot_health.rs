//! Boot crash watchdog and Safe Mode entry (ADR-0025 §6.2).
//!
//! A file-based `boot_attempt` marker records consecutive boot attempts.
//! On each boot the counter is incremented; after a successful health
//! self-check the marker is cleared. If the counter reaches
//! [`CRASH_THRESHOLD`] before being cleared, the next boot is forced into
//! Safe Mode (skips third-party plugins and evolved overlays).
//!
//! The marker is a plain JSON file co-located with the database
//! (`<db_path>.boot_attempt.json`), NOT inside SQLite — this is deliberate:
//! if the database itself is corrupted, the marker must still be readable
//! to trigger the crash-loop fallback.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Number of consecutive failed boots before auto-fallback to Safe Mode.
pub const CRASH_THRESHOLD: u32 = 2;

/// Marker filename suffix appended to the database path.
const MARKER_SUFFIX: &str = ".boot_attempt.json";

/// Persisted boot-attempt marker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootAttempt {
    /// App version that was attempting to boot.
    pub version: String,
    /// Consecutive boot attempts without a confirmed healthy boot.
    pub count: u32,
    /// Unix timestamp (seconds) of the last attempt.
    pub last_ts: u64,
}

impl BootAttempt {
    /// Current unix timestamp in seconds.
    fn now_ts() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// Returns the marker file path for a given database path.
fn marker_path(db_path: &Path) -> PathBuf {
    let s = db_path.to_string_lossy();
    PathBuf::from(format!("{s}{MARKER_SUFFIX}"))
}

/// Reads the current boot attempt marker. Returns `None` if no marker
/// exists or the file is corrupt (treated as "no prior crash").
pub fn read_attempt(db_path: &Path) -> Option<BootAttempt> {
    let path = marker_path(db_path);
    let data = std::fs::read(&path).ok()?;
    serde_json::from_slice(&data).ok()
}

/// Increments the boot attempt counter and writes the marker.
///
/// If an existing marker with the same version is present, `count` is
/// incremented. If the version differs or no marker exists, `count`
/// starts at 1.
pub fn record_attempt(db_path: &Path, version: &str) -> BootAttempt {
    let attempt = match read_attempt(db_path) {
        Some(existing) if existing.version == version => BootAttempt {
            version: version.to_string(),
            count: existing.count.saturating_add(1),
            last_ts: BootAttempt::now_ts(),
        },
        _ => BootAttempt {
            version: version.to_string(),
            count: 1,
            last_ts: BootAttempt::now_ts(),
        },
    };
    let path = marker_path(db_path);
    if let Ok(json) = serde_json::to_vec(&attempt) {
        let _ = std::fs::write(&path, json);
    }
    attempt
}

/// Clears the boot attempt marker. Called after a successful health
/// self-check confirms the boot is stable.
pub fn clear_attempt(db_path: &Path) {
    let _ = std::fs::remove_file(marker_path(db_path));
}

/// Returns `true` if the boot attempt counter has reached the crash
/// threshold, indicating the next boot should be forced into Safe Mode.
pub fn should_safe_mode(db_path: &Path) -> bool {
    read_attempt(db_path)
        .map(|a| a.count >= CRASH_THRESHOLD)
        .unwrap_or(false)
}

/// Result of the post-boot health self-check.
#[derive(Debug, Clone)]
pub struct HealthReport {
    /// SQLite read/write probe succeeded.
    pub sqlite_ok: bool,
    /// All checks passed.
    pub all_ok: bool,
}

impl HealthReport {
    /// A passing health report.
    pub fn ok() -> Self {
        Self {
            sqlite_ok: true,
            all_ok: true,
        }
    }

    /// A failing health report with the given probe results.
    pub fn from_probes(sqlite_ok: bool) -> Self {
        Self {
            sqlite_ok,
            all_ok: sqlite_ok,
        }
    }
}

/// Runs the post-boot health self-check.
///
/// Probes:
/// 1. **SQLite read/write** — opens the database, runs a trivial
///    `SELECT 1` round-trip. If the database cannot be opened or the
///    query fails, the probe fails.
///
/// The WebView mount probe (ADR-0025 §6.2) is only meaningful in the
/// desktop shell and is omitted here; the CLI surface has no WebView.
/// The 30-second steady-state confirmation is likewise a shell concern —
/// the CLI health check is a synchronous best-effort probe at boot time.
pub async fn health_check(db_path: &str) -> HealthReport {
    let sqlite_ok = probe_sqlite(db_path).await;
    HealthReport::from_probes(sqlite_ok)
}

/// SQLite probe: open the database and run `SELECT 1`.
async fn probe_sqlite(db_path: &str) -> bool {
    let path = db_path.to_string();
    tokio::task::spawn_blocking(move || -> bool {
        use crate::store::Db;
        let db = match Db::open(&path) {
            Ok(db) => db,
            Err(_) => return false,
        };
        db.0.query_row("SELECT 1", [], |_| Ok(())).is_ok()
    })
    .await
    .unwrap_or(false)
}

/// App version for boot-attempt tracking. Uses the crate version at
/// compile time; the desktop shell may override with its own version.
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_marker_path() -> PathBuf {
        let dir = std::env::temp_dir();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.join(format!("nuomi_boot_health_test_{ts}.db"))
    }

    #[test]
    fn no_marker_means_no_safe_mode() {
        let path = temp_marker_path();
        assert!(!should_safe_mode(&path));
        clear_attempt(&path); // cleanup
    }

    #[test]
    fn single_attempt_does_not_trigger_safe_mode() {
        let path = temp_marker_path();
        let attempt = record_attempt(&path, "1.0.0");
        assert_eq!(attempt.count, 1);
        assert!(!should_safe_mode(&path));
        clear_attempt(&path);
    }

    #[test]
    fn two_attempts_trigger_safe_mode() {
        let path = temp_marker_path();
        record_attempt(&path, "1.0.0");
        let attempt = record_attempt(&path, "1.0.0");
        assert_eq!(attempt.count, 2);
        assert!(should_safe_mode(&path));
        clear_attempt(&path);
    }

    #[test]
    fn version_change_resets_count() {
        let path = temp_marker_path();
        record_attempt(&path, "1.0.0");
        record_attempt(&path, "1.0.0");
        assert!(should_safe_mode(&path));
        // New version starts fresh
        let attempt = record_attempt(&path, "2.0.0");
        assert_eq!(attempt.count, 1);
        assert!(!should_safe_mode(&path));
        clear_attempt(&path);
    }

    #[test]
    fn clear_resets_marker() {
        let path = temp_marker_path();
        record_attempt(&path, "1.0.0");
        record_attempt(&path, "1.0.0");
        assert!(should_safe_mode(&path));
        clear_attempt(&path);
        assert!(!should_safe_mode(&path));
    }

    #[test]
    fn read_attempt_returns_none_for_missing_file() {
        let path = temp_marker_path();
        assert!(read_attempt(&path).is_none());
    }

    #[test]
    fn read_attempt_returns_none_for_corrupt_file() {
        let path = temp_marker_path();
        let marker = marker_path(&path);
        std::fs::write(&marker, b"not json").unwrap();
        assert!(read_attempt(&path).is_none());
        clear_attempt(&path);
    }

    #[test]
    fn marker_path_has_correct_suffix() {
        let db = PathBuf::from("/tmp/nuomi.db");
        let marker = marker_path(&db);
        assert_eq!(marker.to_string_lossy(), "/tmp/nuomi.db.boot_attempt.json");
    }

    #[test]
    fn health_report_ok_is_all_ok() {
        let r = HealthReport::ok();
        assert!(r.sqlite_ok);
        assert!(r.all_ok);
    }

    #[test]
    fn health_report_from_probes_sqlite_fail() {
        let r = HealthReport::from_probes(false);
        assert!(!r.sqlite_ok);
        assert!(!r.all_ok);
    }
}
