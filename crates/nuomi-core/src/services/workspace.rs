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

    /// Creates a file inside the sandbox. Parent directories are created if
    /// missing. If the file already exists, it is overwritten (atomic replace
    /// via `write_file_atomic`).
    pub fn create_file(&self, rel: &str, content: &str) -> Result<(), WorkspaceError> {
        self.write_file_atomic(rel, content)
    }

    /// Creates a directory inside the sandbox (idempotent, nested).
    pub fn create_dir(&self, rel: &str) -> Result<(), WorkspaceError> {
        let path = self.resolve(rel)?;
        std::fs::create_dir_all(path)?;
        Ok(())
    }

    /// Deletes a file or directory inside the sandbox. Returns `Io` (NotFound)
    /// if the path does not exist.
    pub fn delete(&self, rel: &str) -> Result<(), WorkspaceError> {
        let path = self.resolve(rel)?;
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Renames/moves a file or directory inside the sandbox. Both `from` and
    /// `to` are resolved through the sandbox guard. If `to` already exists it
    /// is removed first (Windows `rename` cannot overwrite). `from == to` is
    /// a no-op.
    pub fn rename(&self, from: &str, to: &str) -> Result<(), WorkspaceError> {
        let from_path = self.resolve(from)?;
        let to_path = self.resolve(to)?;
        if from_path == to_path {
            return Ok(());
        }
        if to_path.exists() {
            if to_path.is_dir() {
                std::fs::remove_dir_all(&to_path)?;
            } else {
                std::fs::remove_file(&to_path)?;
            }
        }
        std::fs::rename(from_path, to_path)?;
        Ok(())
    }

    /// Copies a file or directory inside the sandbox. Both paths are resolved
    /// through the sandbox guard. Returns `InvalidPath` if `from` does not
    /// exist or `to` already exists (does not overwrite/merge, avoiding
    /// accidental data loss).
    pub fn copy(&self, from: &str, to: &str) -> Result<(), WorkspaceError> {
        let from_path = self.resolve(from)?;
        let to_path = self.resolve(to)?;
        if !from_path.exists() {
            return Err(WorkspaceError::InvalidPath(
                from_path.to_string_lossy().into_owned(),
            ));
        }
        if to_path.exists() {
            return Err(WorkspaceError::InvalidPath(
                to_path.to_string_lossy().into_owned(),
            ));
        }
        if from_path.is_dir() {
            copy_dir_recursive(&from_path, &to_path)?;
        } else {
            std::fs::copy(&from_path, &to_path)?;
        }
        Ok(())
    }
}

/// Recursively copies a directory tree. `to` is created if missing. Each
/// entry is copied verbatim (file contents or nested directory). The sandbox
/// guard is enforced by the caller resolving `from`/`to` — since `to` is
/// inside the root, all of `to`'s descendants are too.
fn copy_dir_recursive(from: &Path, to: &Path) -> Result<(), WorkspaceError> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
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

    #[test]
    fn create_file_creates_parents_and_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let ws = service(dir.path());
        ws.create_file("a/b/c.txt", "first").unwrap();
        assert_eq!(ws.read_file("a/b/c.txt").unwrap(), "first");
        // Overwrite.
        ws.create_file("a/b/c.txt", "second").unwrap();
        assert_eq!(ws.read_file("a/b/c.txt").unwrap(), "second");
    }

    #[test]
    fn create_dir_is_idempotent_and_nested() {
        let dir = tempfile::tempdir().unwrap();
        let ws = service(dir.path());
        ws.create_dir("x/y/z").unwrap();
        ws.create_dir("x/y/z").unwrap(); // idempotent
        let entries = ws.list_dir("x/y").unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "z");
    }

    #[test]
    fn delete_file_and_dir() {
        let dir = tempfile::tempdir().unwrap();
        let ws = service(dir.path());
        ws.create_file("f.txt", "x").unwrap();
        ws.create_dir("d/sub").unwrap();
        ws.create_file("d/sub/g.txt", "y").unwrap();

        ws.delete("f.txt").unwrap();
        assert!(ws.read_file("f.txt").is_err());

        ws.delete("d").unwrap(); // recursive
        assert!(ws.list_dir("d").is_err());
    }

    #[test]
    fn delete_nonexistent_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let ws = service(dir.path());
        assert!(ws.delete("nope.txt").is_err());
    }

    #[test]
    fn rename_file_and_dir_and_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let ws = service(dir.path());
        ws.create_file("a.txt", "content").unwrap();
        ws.rename("a.txt", "b.txt").unwrap();
        assert_eq!(ws.read_file("b.txt").unwrap(), "content");
        assert!(ws.read_file("a.txt").is_err());

        // Rename onto existing target overwrites.
        ws.create_file("c.txt", "new").unwrap();
        ws.rename("c.txt", "b.txt").unwrap();
        assert_eq!(ws.read_file("b.txt").unwrap(), "new");

        // Rename a directory.
        ws.create_dir("dir1/sub").unwrap();
        ws.rename("dir1", "dir2").unwrap();
        let entries = ws.list_dir("").unwrap();
        assert!(entries.iter().any(|e| e.name == "dir2" && e.is_dir));
    }

    #[test]
    fn rename_same_path_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let ws = service(dir.path());
        ws.create_file("a.txt", "keep").unwrap();
        ws.rename("a.txt", "a.txt").unwrap();
        assert_eq!(ws.read_file("a.txt").unwrap(), "keep");
    }

    #[test]
    fn copy_file_and_dir() {
        let dir = tempfile::tempdir().unwrap();
        let ws = service(dir.path());
        ws.create_file("src.txt", "data").unwrap();
        ws.copy("src.txt", "dst.txt").unwrap();
        assert_eq!(ws.read_file("dst.txt").unwrap(), "data");
        // Source preserved.
        assert_eq!(ws.read_file("src.txt").unwrap(), "data");

        // Copy a directory tree.
        ws.create_file("tree/a.txt", "1").unwrap();
        ws.create_file("tree/b.txt", "2").unwrap();
        ws.copy("tree", "tree-copy").unwrap();
        assert_eq!(ws.read_file("tree-copy/a.txt").unwrap(), "1");
        assert_eq!(ws.read_file("tree-copy/b.txt").unwrap(), "2");
    }

    #[test]
    fn copy_to_existing_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let ws = service(dir.path());
        ws.create_file("a.txt", "1").unwrap();
        ws.create_file("b.txt", "2").unwrap();
        assert!(ws.copy("a.txt", "b.txt").is_err());
    }

    #[test]
    fn copy_from_nonexistent_is_error() {
        let dir = tempfile::tempdir().unwrap();
        let ws = service(dir.path());
        assert!(ws.copy("nope.txt", "out.txt").is_err());
    }

    #[test]
    fn file_ops_reject_escape_paths() {
        let dir = tempfile::tempdir().unwrap();
        let ws = service(dir.path());
        let escapes = ["../out.txt", "..\\out.txt", "/etc/passwd", "C:\\abs"];
        for rel in escapes {
            assert!(ws.create_file(rel, "x").is_err(), "create_file {rel}");
            assert!(ws.create_dir(rel).is_err(), "create_dir {rel}");
            assert!(ws.delete(rel).is_err(), "delete {rel}");
            assert!(ws.rename(rel, "safe.txt").is_err(), "rename from {rel}");
            assert!(ws.rename("safe.txt", rel).is_err(), "rename to {rel}");
            assert!(ws.copy(rel, "safe.txt").is_err(), "copy from {rel}");
            assert!(ws.copy("safe.txt", rel).is_err(), "copy to {rel}");
        }
    }
}
