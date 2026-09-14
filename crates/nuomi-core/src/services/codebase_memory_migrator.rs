//! Codebase-memory product migrator: `.codebase-memory/` → `.nuomi/codebase-memory/`.
//!
//! Strategy: copy + mark. The legacy `<root>/.codebase-memory/` is recursively
//! copied into `<root>/.nuomi/codebase-memory/`, then a `.migrated` marker is
//! written into the legacy dir. The original is never deleted (rollback safety).
//! Idempotent: a present marker or a non-empty target skips migration.

use std::path::Path;

use thiserror::Error;

use super::nuomi_dir;

const LEGACY_DIR: &str = ".codebase-memory";
const MIGRATED_MARKER: &str = ".migrated";

#[derive(Debug, Error)]
pub enum MigrationError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Outcome of a single `migrate` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationResult {
    /// Migration completed; `count` files were copied.
    Migrated { count: usize },
    /// Skipped: no legacy dir, already migrated, or target non-empty.
    Skipped,
}

/// Returns true when the `.migrated` marker exists in the legacy dir.
pub fn is_migrated(root: &Path) -> bool {
    root.join(LEGACY_DIR).join(MIGRATED_MARKER).exists()
}

/// Migrates `<root>/.codebase-memory/` → `<root>/.nuomi/codebase-memory/`.
///
/// - No legacy dir → `Skipped`.
/// - Already migrated (marker present) → `Skipped`.
/// - Target already non-empty → `Skipped` (merge/rollback safety).
/// - Otherwise: recursive copy + write marker → `Migrated { count }`.
pub fn migrate(root: &Path) -> Result<MigrationResult, MigrationError> {
    let legacy = root.join(LEGACY_DIR);
    if !legacy.exists() {
        return Ok(MigrationResult::Skipped);
    }
    if is_migrated(root) {
        return Ok(MigrationResult::Skipped);
    }
    let target = nuomi_dir::codebase_memory_dir(root);
    if target.exists() && is_dir_non_empty(&target)? {
        return Ok(MigrationResult::Skipped);
    }
    std::fs::create_dir_all(&target)?;
    let count = copy_dir_recursive(&legacy, &target)?;
    // Write marker into the legacy dir (timestamp content for traceability).
    let marker = legacy.join(MIGRATED_MARKER);
    let content = crate::domain::now_ms().to_string();
    std::fs::write(&marker, content)?;
    Ok(MigrationResult::Migrated { count })
}

fn is_dir_non_empty(path: &Path) -> Result<bool, std::io::Error> {
    let mut it = std::fs::read_dir(path)?;
    Ok(it.next().is_some())
}

/// Recursively copies `src` into `dst`, returning the count of files copied.
/// Existing files at `dst` are overwritten (the target was confirmed empty
/// or absent by the caller).
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<usize, std::io::Error> {
    let mut count = 0;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            std::fs::create_dir_all(&to)?;
            count += copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_legacy(root: &Path, files: &[&str]) {
        let dir = root.join(LEGACY_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        for f in files {
            std::fs::write(dir.join(f), b"data").unwrap();
        }
    }

    #[test]
    fn no_legacy_dir_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(migrate(dir.path()).unwrap(), MigrationResult::Skipped);
    }

    #[test]
    fn migrates_copies_and_marks() {
        let dir = tempfile::tempdir().unwrap();
        make_legacy(dir.path(), &["graph.db", "index.json"]);
        let result = migrate(dir.path()).unwrap();
        assert_eq!(result, MigrationResult::Migrated { count: 2 });
        // Target has the files.
        let target = nuomi_dir::codebase_memory_dir(dir.path());
        assert!(target.join("graph.db").exists());
        assert!(target.join("index.json").exists());
        // Marker written.
        assert!(is_migrated(dir.path()));
        // Legacy preserved (not deleted).
        assert!(dir.path().join(LEGACY_DIR).exists());
    }

    #[test]
    fn already_migrated_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        make_legacy(dir.path(), &["graph.db"]);
        migrate(dir.path()).unwrap();
        // Second call is a no-op.
        assert_eq!(migrate(dir.path()).unwrap(), MigrationResult::Skipped);
    }

    #[test]
    fn non_empty_target_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        make_legacy(dir.path(), &["graph.db"]);
        // Pre-populate target.
        let target = nuomi_dir::codebase_memory_dir(dir.path());
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("existing"), b"x").unwrap();
        assert_eq!(migrate(dir.path()).unwrap(), MigrationResult::Skipped);
    }

    #[test]
    fn nested_directories_are_copied() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join(LEGACY_DIR);
        std::fs::create_dir_all(legacy.join("sub")).unwrap();
        std::fs::write(legacy.join("sub").join("deep.db"), b"x").unwrap();
        std::fs::write(legacy.join("top.db"), b"y").unwrap();
        let result = migrate(dir.path()).unwrap();
        assert_eq!(result, MigrationResult::Migrated { count: 2 });
        let target = nuomi_dir::codebase_memory_dir(dir.path());
        assert!(target.join("sub").join("deep.db").exists());
        assert!(target.join("top.db").exists());
    }
}
