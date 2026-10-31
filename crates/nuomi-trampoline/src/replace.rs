//! Cross-platform atomic binary replacement primitives (ADR-0025 §6.1).
//!
//! Used by the trampoline launcher to atomically swap a new binary into
//! place while archiving the old one. The archive-then-move strategy
//! avoids in-place replacement: the old binary is moved to
//! `.nuomi/archive/` first, then the new binary is moved to the target
//! path. If the second move fails, the old binary is restored.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

/// Errors produced by the replacement primitives.
#[derive(Debug, thiserror::Error)]
pub enum ReplaceError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("process {pid} still alive after {timeout_secs}s timeout")]
    ProcessAlive { pid: u32, timeout_secs: u64 },
}

/// Computes the SHA-256 hex digest of a file.
pub fn file_sha256(path: &Path) -> io::Result<String> {
    let data = fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Ok(format!("{:x}", hasher.finalize()))
}

/// Verifies that a file's SHA-256 matches the expected hex digest.
pub fn verify_hash(path: &Path, expected: &str) -> Result<(), ReplaceError> {
    let actual = file_sha256(path)?;
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(ReplaceError::HashMismatch {
            expected: expected.to_string(),
            actual,
        })
    }
}

/// Waits for a process to terminate. Polls every 100ms up to `timeout`.
pub fn wait_for_pid(pid: u32, timeout: Duration) -> Result<(), ReplaceError> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if !process_exists(pid) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(ReplaceError::ProcessAlive {
        pid,
        timeout_secs: timeout.as_secs(),
    })
}

/// Checks whether a process with the given PID is still running.
#[cfg(unix)]
fn process_exists(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Checks whether a process with the given PID is still running.
#[cfg(windows)]
fn process_exists(pid: u32) -> bool {
    let output = std::process::Command::new("tasklist")
        .arg("/FI")
        .arg(format!("PID eq {pid}"))
        .arg("/NH")
        .output();
    match output {
        Ok(o) => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.contains(&pid.to_string())
        }
        Err(_) => false,
    }
}

/// Atomically replaces `target` with `new_binary`, archiving the old
/// binary to `archive_dir`.
///
/// Strategy (ADR-0025 §6.1):
/// 1. If `target` exists, move it to `archive_dir/<name>.<timestamp>`.
/// 2. Move `new_binary` to `target`.
/// 3. If step 2 fails, attempt to restore the old binary from the archive.
///
/// On POSIX, if `new_binary` and `target` are on different filesystems,
/// `rename` fails with `EXDEV` — we fall back to copy + rename.
pub fn atomic_replace(
    new_binary: &Path,
    target: &Path,
    archive_dir: &Path,
) -> Result<PathBuf, ReplaceError> {
    fs::create_dir_all(archive_dir)?;

    // Step 1: archive the old binary if it exists.
    let archive_path = if target.exists() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "binary".to_string());
        let archive = archive_dir.join(format!("{name}.{ts}"));
        fs::rename(target, &archive)?;
        Some(archive)
    } else {
        None
    };

    // Step 2: move the new binary to the target path.
    match fs::rename(new_binary, target) {
        Ok(()) => Ok(archive_path.unwrap_or_else(|| PathBuf::from(""))),
        Err(e) => {
            // Try cross-filesystem fallback: copy + rename.
            if is_cross_device_err(&e) {
                match copy_and_rename(new_binary, target) {
                    Ok(()) => Ok(archive_path.unwrap_or_else(|| PathBuf::from(""))),
                    Err(e2) => {
                        if let Some(ref archive) = archive_path {
                            let _ = fs::rename(archive, target);
                        }
                        Err(e2)
                    }
                }
            } else {
                // Restore the old binary if we archived it.
                if let Some(ref archive) = archive_path {
                    let _ = fs::rename(archive, target);
                }
                Err(ReplaceError::Io(e))
            }
        }
    }
}

/// Returns `true` if the error is a cross-device link error (`EXDEV`).
#[cfg(unix)]
fn is_cross_device_err(e: &io::Error) -> bool {
    e.raw_os_error() == Some(18) // EXDEV
}

#[cfg(not(unix))]
fn is_cross_device_err(_e: &io::Error) -> bool {
    false
}

/// Copies `src` to a temp file in the same directory as `dst`, then
/// atomically renames the temp file to `dst`.
fn copy_and_rename(src: &Path, dst: &Path) -> Result<(), ReplaceError> {
    let dst_dir = dst.parent().unwrap_or(Path::new("."));
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dst_dir.join(format!(".nuomi_tmp_{ts}"));
    if let Err(e) = fs::copy(src, &tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(ReplaceError::Io(e));
    }
    if let Err(e) = fs::rename(&tmp, dst) {
        let _ = fs::remove_file(&tmp);
        return Err(ReplaceError::Io(e));
    }
    Ok(())
}

/// Launches the replaced binary with the given extra arguments.
pub fn launch(target: &Path, args: &[String]) -> io::Result<()> {
    let mut cmd = std::process::Command::new(target);
    cmd.args(args);
    cmd.spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn file_sha256_known_value() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(b"hello world").unwrap();
        f.flush().unwrap();
        let hash = file_sha256(&path).unwrap();
        // SHA-256("hello world") = b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn verify_hash_matches() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, b"hello world").unwrap();
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert!(verify_hash(&path, expected).is_ok());
    }

    #[test]
    fn verify_hash_mismatch() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, b"hello world").unwrap();
        let expected = "0000000000000000000000000000000000000000000000000000000000000000";
        let err = verify_hash(&path, expected).unwrap_err();
        assert!(matches!(err, ReplaceError::HashMismatch { .. }));
    }

    #[test]
    fn verify_hash_case_insensitive() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");
        fs::write(&path, b"hello world").unwrap();
        let expected = "B94D27B9934D3E08A52E52D7DA7DABFAC484EFE37A5380EE9088F7ACE2EFCDE9";
        assert!(verify_hash(&path, expected).is_ok());
    }

    #[test]
    fn atomic_replace_archives_old_and_installs_new() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("app");
        let new_binary = dir.path().join("new");
        let archive_dir = dir.path().join("archive");

        fs::write(&target, b"old content").unwrap();
        fs::write(&new_binary, b"new content").unwrap();

        let archive_path = atomic_replace(&new_binary, &target, &archive_dir).unwrap();

        // Target now has new content.
        assert_eq!(fs::read(&target).unwrap(), b"new content");
        // Old binary was archived.
        assert!(archive_path.exists());
        assert_eq!(fs::read(&archive_path).unwrap(), b"old content");
        // New binary staging path is gone.
        assert!(!new_binary.exists());
    }

    #[test]
    fn atomic_replace_no_existing_target() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("app");
        let new_binary = dir.path().join("new");
        let archive_dir = dir.path().join("archive");

        fs::write(&new_binary, b"new content").unwrap();

        let archive_path = atomic_replace(&new_binary, &target, &archive_dir).unwrap();

        assert_eq!(fs::read(&target).unwrap(), b"new content");
        // No old binary to archive.
        assert!(archive_path.as_os_str().is_empty());
    }

    #[test]
    fn atomic_replace_restores_on_failure() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("app");
        let new_binary = dir.path().join("new");
        let archive_dir = dir.path().join("archive");

        fs::write(&target, b"old content").unwrap();
        // new_binary doesn't exist — rename will fail.

        let result = atomic_replace(&new_binary, &target, &archive_dir);
        assert!(result.is_err());
        // Old binary should be restored.
        assert_eq!(fs::read(&target).unwrap(), b"old content");
    }

    #[test]
    fn wait_for_nonexistent_pid_returns_immediately() {
        // PID 0 is the scheduler on POSIX / System Idle Process on Windows.
        // Use a very high PID that almost certainly doesn't exist.
        let result = wait_for_pid(999_999, Duration::from_secs(1));
        // On most systems this PID doesn't exist, so it should succeed.
        // PID 999_999 almost certainly doesn't exist on any real system.
        assert!(result.is_ok(), "expected PID 999999 to not exist");
    }
}
