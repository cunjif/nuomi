//! Workspace path guard: canonicalization + blacklist enforcement.
//!
//! Canonicalizes a user-selected directory path and rejects OS-critical
//! locations (system roots, Program Files, user home parent, `.nuomi` itself)
//! so the app never sandboxes a dangerous root.

use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PathGuardError {
    #[error("path does not exist or is not a directory: {0}")]
    InvalidPath(String),
    #[error("path is blacklisted: {0}")]
    Blacklisted(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Canonicalizes `path`: resolves symlinks, `..`, relative segments, and
/// trailing separators to a normalized absolute path. Returns `InvalidPath`
/// when the path does not exist or is not a directory.
pub fn canonicalize(path: &Path) -> Result<PathBuf, PathGuardError> {
    let resolved = std::fs::canonicalize(path)
        .map_err(|_| PathGuardError::InvalidPath(path.to_string_lossy().to_string()))?;
    if !resolved.is_dir() {
        return Err(PathGuardError::InvalidPath(
            resolved.to_string_lossy().to_string(),
        ));
    }
    Ok(resolved)
}

/// Returns true when `path` (already canonicalized) matches a blacklisted
/// OS-critical location or ends with `.nuomi` (the app's own product dir).
pub fn is_blacklisted(path: &Path) -> bool {
    let s = path.to_string_lossy();
    // `.nuomi` itself is never a valid workspace root.
    if path
        .file_name()
        .map(|name| name == ".nuomi")
        .unwrap_or(false)
    {
        return true;
    }
    BLACKLIST.iter().any(|&bad| s == bad)
}

/// Platform-specific blacklist of OS-critical directories.
#[cfg(target_os = "windows")]
const BLACKLIST: &[&str] = &[
    "C:\\",
    "D:\\",
    "E:\\",
    "F:\\",
    "C:\\Windows",
    "C:\\Windows\\System32",
    "C:\\Windows\\System",
    "C:\\Program Files",
    "C:\\Program Files (x86)",
    "C:\\ProgramData",
    "C:\\Users",
];

#[cfg(target_os = "macos")]
const BLACKLIST: &[&str] = &[
    "/", "/etc", "/var", "/usr", "/bin", "/sbin", "/sys", "/proc", "/dev", "/opt", "/Users",
    "/home",
];

#[cfg(target_os = "linux")]
const BLACKLIST: &[&str] = &[
    "/", "/etc", "/var", "/usr", "/bin", "/sbin", "/sys", "/proc", "/dev", "/run", "/opt", "/boot",
    "/lib", "/lib64", "/home", "/root",
];

/// Fallback for non-{windows,macos,linux} targets: empty blacklist.
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
const BLACKLIST: &[&str] = &[];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nuomi_dir_name_is_blacklisted() {
        let p = Path::new("C:\\projects\\demo\\.nuomi");
        assert!(is_blacklisted(p));
    }

    #[test]
    fn blacklist_matches_os_critical_paths() {
        for &bad in BLACKLIST {
            assert!(is_blacklisted(Path::new(bad)), "must blacklist {bad}");
        }
    }

    #[test]
    fn normal_path_is_not_blacklisted() {
        let p = Path::new("C:\\projects\\nuomi");
        assert!(!is_blacklisted(p));
    }

    #[test]
    fn canonicalize_rejects_nonexistent() {
        let p = Path::new("Z:\\definitely\\does\\not\\exist\\xyz123");
        assert!(matches!(
            canonicalize(p),
            Err(PathGuardError::InvalidPath(_))
        ));
    }

    #[test]
    fn canonicalize_resolves_existing_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let resolved = canonicalize(dir.path()).unwrap();
        assert!(resolved.is_absolute());
        assert!(resolved.is_dir());
    }

    #[test]
    fn canonicalize_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let c1 = canonicalize(dir.path()).unwrap();
        let c2 = canonicalize(&c1).unwrap();
        assert_eq!(c1, c2);
    }
}
