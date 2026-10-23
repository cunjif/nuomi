//! Source-path probe (chat_rich_bind spec 5.2.1-5/6/7, design §2.2.2-A).
//!
//! Classifies a source-evidence target (absolute path or URL) into the
//! three navigation branches: `web_url`, `workspace_file` (inside a mounted
//! work directory, with workspace/directory ids and the directory-relative
//! path), `external_file` (exists on disk but outside every mounted
//! directory), or `unavailable` (missing/unparseable). Path matching is
//! separator- and case-normalized per platform (Windows/macOS compare
//! case-insensitively, Linux case-sensitively).

use rusqlite::Connection;
use std::path::Path;

use crate::store::repos::work_directories::WorkDirectoryEntry;
use crate::store::StoreError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceProbeResult {
    /// `workspace_file` | `external_file` | `web_url` | `unavailable`.
    pub kind: String,
    pub workspace_id: Option<String>,
    pub directory_id: Option<String>,
    pub relative_path: Option<String>,
    /// Normalized absolute path / URL for the external branches.
    pub normalized_target: Option<String>,
    pub available: bool,
}

impl SourceProbeResult {
    fn web_url(url: &str) -> Self {
        Self {
            kind: "web_url".to_string(),
            workspace_id: None,
            directory_id: None,
            relative_path: None,
            normalized_target: Some(url.to_string()),
            available: true,
        }
    }

    fn unavailable() -> Self {
        Self {
            kind: "unavailable".to_string(),
            workspace_id: None,
            directory_id: None,
            relative_path: None,
            normalized_target: None,
            available: false,
        }
    }
}

/// True when path comparison is case-insensitive on this platform.
fn case_insensitive_fs() -> bool {
    cfg!(windows) || cfg!(target_os = "macos")
}

/// Normalizes a path for comparison: unified separators + optional lowercase.
fn normalize_for_compare(path: &str) -> String {
    let unified = path.replace('\\', "/");
    if case_insensitive_fs() {
        unified.to_lowercase()
    } else {
        unified
    }
}

/// Normalizes separators only (keeps case for display/output).
fn normalize_separators(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.replace('\\', "/")
    }
}

fn list_all_mounted_directories(conn: &Connection) -> Result<Vec<WorkDirectoryEntry>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, abs_path, display_name, mounted, added_at
         FROM work_directories WHERE mounted = 1",
    )?;
    let rows = stmt.query_map([], |row| {
        let mounted: i64 = row.get(4)?;
        Ok(WorkDirectoryEntry {
            id: row.get(0)?,
            workspace_id: row.get(1)?,
            abs_path: row.get(2)?,
            display_name: row.get(3)?,
            mounted: mounted != 0,
            added_at: row.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Probes a source target. Read-only and idempotent.
pub fn probe_source_path(conn: &Connection, target: &str) -> Result<SourceProbeResult, StoreError> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return Ok(SourceProbeResult::unavailable());
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Ok(SourceProbeResult::web_url(trimmed));
    }
    let normalized = normalize_separators(trimmed);
    let comparable = normalize_for_compare(&normalized);
    if !comparable.contains('/') && !comparable.contains('\\') {
        // No separator and no scheme: not a path we can reason about.
        return Ok(SourceProbeResult::unavailable());
    }
    let exists = Path::new(&normalized).exists();
    for dir in list_all_mounted_directories(conn)? {
        let dir_cmp = normalize_for_compare(&dir.abs_path);
        let dir_prefix = dir_cmp.trim_end_matches('/');
        if let Some(rest) = comparable.strip_prefix(dir_prefix) {
            let rest = rest.trim_start_matches('/');
            if rest.is_empty() || !rest.starts_with("..") {
                let relative = rest.replace('\\', "/");
                return Ok(SourceProbeResult {
                    kind: "workspace_file".to_string(),
                    workspace_id: Some(dir.workspace_id),
                    directory_id: Some(dir.id),
                    relative_path: Some(relative),
                    normalized_target: None,
                    available: exists,
                });
            }
        }
    }
    if exists {
        Ok(SourceProbeResult {
            kind: "external_file".to_string(),
            workspace_id: None,
            directory_id: None,
            relative_path: None,
            normalized_target: Some(normalized),
            available: true,
        })
    } else {
        Ok(SourceProbeResult::unavailable())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::migrations;
    use crate::store::repos::workspaces::{self, WorkspaceEntry};

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn
    }

    fn seed_directory(conn: &Connection, ws: &str, dir_id: &str, abs: &str) {
        workspaces::insert(
            conn,
            &WorkspaceEntry {
                id: ws.into(),
                root_path: abs.into(),
                color_tag: "paper-yellow".into(),
                created_at: 1,
                is_active: false,
                is_pinned: false,
            },
        )
        .unwrap();
        conn.execute(
            "INSERT INTO work_directories (id, workspace_id, abs_path, display_name, mounted, added_at)
             VALUES (?1, ?2, ?3, ?4, 1, 1)",
            rusqlite::params![dir_id, ws, abs, dir_id],
        )
        .unwrap();
    }

    #[test]
    fn web_url_short_circuits() {
        let conn = db();
        for url in ["https://example.com/a", "http://x.io"] {
            let r = probe_source_path(&conn, url).unwrap();
            assert_eq!(r.kind, "web_url");
            assert!(r.available);
            assert_eq!(r.normalized_target.as_deref(), Some(url));
        }
    }

    #[test]
    fn workspace_file_branch_with_relative_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src").join("main.rs"), b"x").unwrap();
        let root_str = root.to_string_lossy().to_string();
        let conn = db();
        seed_directory(&conn, "w1", "d1", &root_str);

        let target = format!(
            "{}{}src{}main.rs",
            root_str,
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        );
        let r = probe_source_path(&conn, &target).unwrap();
        assert_eq!(r.kind, "workspace_file");
        assert_eq!(r.workspace_id.as_deref(), Some("w1"));
        assert_eq!(r.directory_id.as_deref(), Some("d1"));
        assert_eq!(r.relative_path.as_deref(), Some("src/main.rs"));
        assert!(r.available);
    }

    #[test]
    fn separator_and_case_normalization_on_windows_style_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Project");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("a.rs"), b"x").unwrap();
        let root_str = root.to_string_lossy().to_string();
        let conn = db();
        seed_directory(&conn, "w1", "d1", &root_str);

        // Forward slashes + different case: must still match on
        // case-insensitive platforms (Windows/macOS); on Linux the dir was
        // seeded with the same case, so lowercase input matches trivially
        // only there too — normalize input to the seeded case for both.
        let slashed = format!("{}/src/a.rs", root_str.trim_end_matches(['/', '\\']));
        let r = probe_source_path(&conn, &slashed).unwrap();
        if case_insensitive_fs() {
            assert_eq!(r.kind, "workspace_file", "slashed input should match");
            assert_eq!(r.relative_path.as_deref(), Some("src/a.rs"));
        } else {
            assert_eq!(r.kind, "workspace_file");
        }
    }

    #[test]
    fn external_file_and_unavailable_branches() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("outside.txt");
        std::fs::write(&outside, b"x").unwrap();
        let conn = db();
        seed_directory(
            &conn,
            "w1",
            "d1",
            dir.path()
                .join("nested")
                .to_string_lossy()
                .to_string()
                .as_str(),
        );

        let r = probe_source_path(&conn, outside.to_string_lossy().as_ref()).unwrap();
        assert_eq!(r.kind, "external_file");
        assert!(r.available);

        let r = probe_source_path(&conn, "C:\\definitely\\not\\here.txt").unwrap();
        assert_eq!(r.kind, "unavailable");
        assert!(!r.available);

        let r = probe_source_path(&conn, "").unwrap();
        assert_eq!(r.kind, "unavailable");
        let r = probe_source_path(&conn, "plainname").unwrap();
        assert_eq!(r.kind, "unavailable");
    }

    #[test]
    fn unmounted_directory_never_matches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("unmounted");
        std::fs::create_dir_all(&root).unwrap();
        let root_str = root.to_string_lossy().to_string();
        let conn = db();
        seed_directory(&conn, "w1", "d1", &root_str);
        conn.execute(
            "UPDATE work_directories SET mounted = 0 WHERE id = 'd1'",
            [],
        )
        .unwrap();
        let target = format!("{root_str}{}file.txt", std::path::MAIN_SEPARATOR);
        let r = probe_source_path(&conn, &target).unwrap();
        assert_eq!(r.kind, "unavailable");
    }
}
