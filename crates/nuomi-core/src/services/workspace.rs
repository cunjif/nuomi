//! WorkspaceService (SPEC ui-m1 D4 / AC4): sandboxed file access confined to
//! a configured root. Every path is resolved through canonicalization of the
//! deepest existing ancestor, so `..` traversal, absolute paths outside the
//! root and symlink/junction escapes are all rejected. Writes are atomic:
//! temp file + rename (with a pre-remove on Windows where rename cannot
//! overwrite an existing target).

use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WorkspaceError {
    #[error("path escapes workspace root: {path}")]
    Escape { path: String },
    #[error("invalid path: {0}")]
    InvalidPath(String),
    #[error("not a directory: {0}")]
    NotADirectory(String),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

/// One entry of `list_dir`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

/// Sandboxed access to one workspace root.
#[derive(Debug, Clone)]
pub struct WorkspaceService {
    /// Canonical root — every resolved path must stay inside it.
    root: PathBuf,
}

impl WorkspaceService {
    /// Creates the service; the root must exist and is canonicalized once.
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, WorkspaceError> {
        let root = root.into();
        if !root.is_dir() {
            return Err(WorkspaceError::NotADirectory(
                root.to_string_lossy().into_owned(),
            ));
        }
        Ok(Self {
            root: root.canonicalize()?,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolves `rel` against the root, rejecting any escape.
    ///
    /// Canonicalize walks up to the deepest ancestor that exists (the target
    /// itself may not exist yet for writes), then re-attaches the remainder.
    /// Because canonicalization resolves symlinks/junctions, a final
    /// `starts_with(canonical_root)` check catches all three escape classes:
    /// `..` traversal, absolute paths outside the root, and link-based
    /// escapes. (Windows symlink creation needs privileges, so tests cover
    /// the first two directly; the check here is link-agnostic.)
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, WorkspaceError> {
        let trimmed = rel.trim();
        if trimmed.is_empty() {
            return Ok(self.root.clone());
        }
        let rel_path = Path::new(trimmed);
        if rel_path.is_absolute() || has_windows_drive_prefix(trimmed) {
            return Err(WorkspaceError::Escape {
                path: trimmed.to_string(),
            });
        }
        let candidate = self.root.join(rel_path);
        let canonical = canonicalize_ancestor(&candidate)?;
        if !canonical.starts_with(&self.root) {
            return Err(WorkspaceError::Escape {
                path: trimmed.to_string(),
            });
        }
        Ok(canonical)
    }

    /// Lists a directory inside the sandbox.
    pub fn list_dir(&self, rel: &str) -> Result<Vec<FileEntry>, WorkspaceError> {
        let dir = self.resolve(rel)?;
        if !dir.is_dir() {
            return Err(WorkspaceError::NotADirectory(
                dir.to_string_lossy().into_owned(),
            ));
        }
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            entries.push(FileEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                size: meta.len(),
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    /// Reads a UTF-8 file inside the sandbox.
    pub fn read_file(&self, rel: &str) -> Result<String, WorkspaceError> {
        let path = self.resolve(rel)?;
        if path.is_dir() {
            return Err(WorkspaceError::InvalidPath(
                path.to_string_lossy().into_owned(),
            ));
        }
        Ok(std::fs::read_to_string(path)?)
    }

    /// Atomic write: content goes to a temp file in the same directory, then
    /// replaces the target via rename. On Windows `rename` fails when the
    /// target exists, so the old file is removed in between (documented,
    /// tested behavior; the vulnerable window is a single syscall wide).
    pub fn write_file_atomic(&self, rel: &str, content: &str) -> Result<(), WorkspaceError> {
        let target = self.resolve(rel)?;
        if target.is_dir() {
            return Err(WorkspaceError::InvalidPath(
                target.to_string_lossy().into_owned(),
            ));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = target.with_extension(format!("{}tmp", uuid::Uuid::now_v7().simple()));
        std::fs::write(&tmp, content)?;
        if target.exists() {
            std::fs::remove_file(&target)?;
        }
        match std::fs::rename(&tmp, &target) {
            Ok(()) => Ok(()),
            Err(e) => {
                // best effort cleanup of the orphaned temp file
                let _ = std::fs::remove_file(&tmp);
                Err(e.into())
            }
        }
    }
}

fn has_windows_drive_prefix(rel: &str) -> bool {
    let bytes = rel.as_bytes();
    bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic()
}

/// Canonicalizes the deepest existing ancestor of `path`, appending the
/// non-existing remainder verbatim.
fn canonicalize_ancestor(path: &Path) -> Result<PathBuf, WorkspaceError> {
    if let Ok(canon) = path.canonicalize() {
        return Ok(canon);
    }
    let parent = path
        .parent()
        .ok_or_else(|| WorkspaceError::InvalidPath(path.to_string_lossy().into_owned()))?;
    let mut canon_parent = if parent == path {
        return Err(WorkspaceError::InvalidPath(
            path.to_string_lossy().into_owned(),
        ));
    } else {
        canonicalize_ancestor(parent)?
    };
    match path.file_name() {
        Some(name) => canon_parent.push(name),
        None => {
            return Err(WorkspaceError::InvalidPath(
                path.to_string_lossy().into_owned(),
            ))
        }
    }
    Ok(canon_parent)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(dir: &Path) -> WorkspaceService {
        WorkspaceService::new(dir).unwrap()
    }

    #[test]
    fn read_write_list_roundtrip_inside_root() {
        let dir = tempfile::tempdir().unwrap();
        let ws = service(dir.path());

        ws.write_file_atomic("notes/hello.txt", "hi nuomi").unwrap();
        assert_eq!(ws.read_file("notes/hello.txt").unwrap(), "hi nuomi");

        // overwrite is atomic-replace, not append/duplicate
        ws.write_file_atomic("notes/hello.txt", "v2").unwrap();
        assert_eq!(ws.read_file("notes/hello.txt").unwrap(), "v2");
        // no temp litter left behind
        let names: Vec<String> = ws
            .list_dir("notes")
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert_eq!(names, vec!["hello.txt".to_string()]);

        let entries = ws.list_dir("").unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "notes");

        let file_only = ws.list_dir("notes").unwrap();
        assert!(!file_only[0].is_dir);
        assert_eq!(file_only[0].size, 2);
    }

    #[test]
    fn table_escape_paths_are_rejected() {
        let cases: &[&str] = &[
            "../escape.txt",
            "..\\escape.txt",
            "sub/../../outside.txt",
            "a/b/../../../out",
            "C:\\abs\\path",
            "/etc/passwd",
            "D:/top.txt",
        ];
        for rel in cases {
            let dir = tempfile::tempdir().unwrap();
            let ws = service(dir.path());
            let err = ws.resolve(rel).expect_err(rel);
            assert!(
                matches!(err, WorkspaceError::Escape { .. }),
                "{rel} → {err:?}"
            );
            // and neither read nor write can be tricked either
            assert!(ws.read_file(rel).is_err());
            assert!(ws.write_file_atomic(rel, "x").is_err());
        }
    }

    #[test]
    fn escape_via_symlink_is_rejected_by_canonicalization() {
        // NOTE: creating real symlinks on Windows requires privileges, so this
        // case uses a directory junction-free equivalent: we verify that the
        // canonical starts_with guard is what rejects escapes by planting a
        // deep `..` chain under an existing subdir (canonicalize resolves it).
        let dir = tempfile::tempdir().unwrap();
        let ws = service(dir.path());
        std::fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
        // Four levels up from a/b/c lands outside the root.
        assert!(matches!(
            ws.resolve("a/b/c/../../../../secret.txt"),
            Err(WorkspaceError::Escape { .. })
        ));
    }

    #[test]
    fn nonexistent_target_within_root_resolves_for_writes() {
        let dir = tempfile::tempdir().unwrap();
        let ws = service(dir.path());
        let p = ws.resolve("deep/nested/new-file.txt").unwrap();
        assert!(p.starts_with(ws.root()));
        ws.write_file_atomic("deep/nested/new-file.txt", "data")
            .unwrap();
        assert_eq!(ws.read_file("deep/nested/new-file.txt").unwrap(), "data");
    }

    #[test]
    fn root_must_exist_and_be_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(WorkspaceService::new(dir.path().join("missing")).is_err());
        let file = dir.path().join("f.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(matches!(
            WorkspaceService::new(&file),
            Err(WorkspaceError::NotADirectory(_))
        ));
    }

    #[test]
    fn list_dir_on_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let ws = service(dir.path());
        ws.write_file_atomic("f.txt", "x").unwrap();
        assert!(matches!(
            ws.list_dir("f.txt"),
            Err(WorkspaceError::NotADirectory(_))
        ));
    }
}
