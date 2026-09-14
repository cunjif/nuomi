//! `.nuomi/` directory layout: the single product root under each workspace.
//!
//! `<workspace>/.nuomi/` is created idempotently when a workspace is registered.
//! Sub-directory paths (e.g. `codebase-memory/`) are computed but not eagerly
//! created — callers `create_dir_all` on demand to avoid empty-dir noise.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// The directory name placed under each workspace root.
pub const NUOMI_DIR_NAME: &str = ".nuomi";

#[derive(Debug, Error)]
pub enum NuomiDirError {
    #[error(".nuomi path exists but is not a directory: {0}")]
    Conflict(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Idempotently ensures `<root>/.nuomi/` exists as a directory. Returns the
/// full path. Fails with `Conflict` when a non-directory file already sits at
/// that location (never overwrites).
pub fn ensure_nuomi_dir(root: &Path) -> Result<PathBuf, NuomiDirError> {
    let nuomi = root.join(NUOMI_DIR_NAME);
    if nuomi.exists() && !nuomi.is_dir() {
        return Err(NuomiDirError::Conflict(
            nuomi.to_string_lossy().to_string(),
        ));
    }
    std::fs::create_dir_all(&nuomi)?;
    Ok(nuomi)
}

/// Returns the path `<root>/.nuomi/codebase-memory/` without creating it.
/// Callers create on demand via `std::fs::create_dir_all`.
pub fn codebase_memory_dir(root: &Path) -> PathBuf {
    root.join(NUOMI_DIR_NAME).join("codebase-memory")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_creates_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let p1 = ensure_nuomi_dir(dir.path()).unwrap();
        assert!(p1.is_dir());
        // Second call is a no-op (idempotent).
        let p2 = ensure_nuomi_dir(dir.path()).unwrap();
        assert_eq!(p1, p2);
    }

    #[test]
    fn ensure_fails_when_file_blocks_dir() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join(NUOMI_DIR_NAME);
        std::fs::write(&blocker, b"x").unwrap();
        assert!(matches!(
            ensure_nuomi_dir(dir.path()),
            Err(NuomiDirError::Conflict(_))
        ));
    }

    #[test]
    fn codebase_memory_dir_path_is_correct() {
        let root = Path::new("C:\\ws");
        let p = codebase_memory_dir(root);
        assert_eq!(p, Path::new("C:\\ws\\.nuomi\\codebase-memory"));
        // Not created.
        assert!(!p.exists());
    }
}
