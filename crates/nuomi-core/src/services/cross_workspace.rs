//! Cross-workspace operations service.
//!
//! Provides read-only collaboration across open workspaces: global search,
//! file reference injection, and file comparison. All operations are sandboxed
//! to the source workspace's root path and never write across workspace
//! boundaries.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use thiserror::Error;

use super::workspace_registry_v2::compat_roots;
use crate::store::repos::workspace_open_state;
use crate::store::repos::workspaces;
use crate::store::{migrations, Db, StoreError};

#[derive(Debug, Error)]
pub enum CrossWorkspaceError {
    #[error("workspace not found: {0}")]
    NotFound(String),
    #[error("workspace directory missing: {0}")]
    DirectoryMissing(String),
    #[error("file not found: {0}")]
    FileNotFound(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
}

/// A search result grouped by workspace.
#[derive(Debug, Clone)]
pub struct CrossSearchResultGroup {
    pub workspace_id: String,
    pub workspace_name: String,
    pub matches: Vec<FileMatch>,
}

/// A single file match within a workspace.
#[derive(Debug, Clone)]
pub struct FileMatch {
    pub relative_path: String,
    pub match_type: MatchType,
}

/// Type of search match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchType {
    FileName,
    FileContent,
}

/// Result of a cross-workspace search.
#[derive(Debug, Clone)]
pub struct CrossSearchOutcome {
    pub groups: Vec<CrossSearchResultGroup>,
    pub skipped_workspace_ids: Vec<String>,
}

/// A read-only file reference from one workspace injected into a target session.
#[derive(Debug, Clone)]
pub struct FileReference {
    pub source_workspace_id: String,
    pub source_relative_path: String,
    pub content_snapshot: String,
}

/// Result of comparing two files across workspaces.
#[derive(Debug, Clone)]
pub struct DiffResult {
    pub workspace_a_id: String,
    pub workspace_b_id: String,
    pub file_a_path: String,
    pub file_b_path: String,
    pub content_a: String,
    pub content_b: String,
    pub is_identical: bool,
}

/// The cross-workspace service.
#[derive(Clone)]
pub struct CrossWorkspaceService {
    db_path: PathBuf,
}

impl CrossWorkspaceService {
    pub fn new(db_path: PathBuf) -> Self {
        Self { db_path }
    }

    fn conn(&self) -> Result<Connection, CrossWorkspaceError> {
        let db = Db::open(&self.db_path.to_string_lossy())?;
        migrations::run(&db.0)?;
        Ok(db.0)
    }

    /// Searches across all open workspaces with reachable directories.
    /// Returns results grouped by workspace; skips workspaces whose directories
    /// are missing (listed in `skipped_workspace_ids`).
    pub fn search_all_open(
        &self,
        query: &str,
        match_content: bool,
    ) -> Result<CrossSearchOutcome, CrossWorkspaceError> {
        let conn = self.conn()?;
        let open_state = workspace_open_state::list(&conn)?;
        let mut groups = Vec::new();
        let mut skipped = Vec::new();

        for state_row in &open_state {
            let ws = match workspaces::find_by_id(&conn, &state_row.workspace_id)? {
                Some(w) => w,
                None => {
                    skipped.push(state_row.workspace_id.clone());
                    continue;
                }
            };

            // ADR-0021 root_path 兼容读取：v2 工作区从 work_directories 派生
            // 根目录（可能多个，全部搜索）；v1 回退 root_path。
            let roots = compat_roots(&conn, &ws)?;
            if roots.is_empty() {
                skipped.push(ws.id.clone());
                continue;
            }

            let mut matches = Vec::new();
            for root in &roots {
                matches.extend(search_in_workspace(root, query, match_content)?);
            }
            matches.truncate(100);
            if !matches.is_empty() {
                let name = roots[0]
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| ws.id.clone());
                groups.push(CrossSearchResultGroup {
                    workspace_id: ws.id.clone(),
                    workspace_name: name,
                    matches,
                });
            }
        }

        Ok(CrossSearchOutcome {
            groups,
            skipped_workspace_ids: skipped,
        })
    }

    /// Creates a read-only file reference: reads a file snapshot from the source
    /// workspace (sandboxed to its root) for injection into a target session.
    /// Never writes across workspace boundaries.
    pub fn create_file_reference(
        &self,
        source_workspace_id: &str,
        file_path: &str,
    ) -> Result<FileReference, CrossWorkspaceError> {
        let conn = self.conn()?;
        let ws = workspaces::find_by_id(&conn, source_workspace_id)?
            .ok_or_else(|| CrossWorkspaceError::NotFound(source_workspace_id.to_string()))?;

        let full_path = resolve_file(&conn, &ws, file_path)?;

        let content = std::fs::read_to_string(&full_path)?;
        Ok(FileReference {
            source_workspace_id: source_workspace_id.to_string(),
            source_relative_path: file_path.to_string(),
            content_snapshot: content,
        })
    }

    /// Compares two files across workspaces (read-only, never modifies either).
    pub fn compare_files(
        &self,
        workspace_a: &str,
        file_a: &str,
        workspace_b: &str,
        file_b: &str,
    ) -> Result<DiffResult, CrossWorkspaceError> {
        let conn = self.conn()?;
        let ws_a = workspaces::find_by_id(&conn, workspace_a)?
            .ok_or_else(|| CrossWorkspaceError::NotFound(workspace_a.to_string()))?;
        let ws_b = workspaces::find_by_id(&conn, workspace_b)?
            .ok_or_else(|| CrossWorkspaceError::NotFound(workspace_b.to_string()))?;

        let path_a = resolve_file(&conn, &ws_a, file_a)?;
        let path_b = resolve_file(&conn, &ws_b, file_b)?;

        let content_a = std::fs::read_to_string(&path_a)?;
        let content_b = std::fs::read_to_string(&path_b)?;

        Ok(DiffResult {
            workspace_a_id: workspace_a.to_string(),
            workspace_b_id: workspace_b.to_string(),
            file_a_path: file_a.to_string(),
            file_b_path: file_b.to_string(),
            is_identical: content_a == content_b,
            content_a,
            content_b,
        })
    }
}

/// Resolves `file_path` inside a workspace's compat roots: v2 multi-directory
/// workspaces probe every available directory in `added_at` order and the
/// first hit wins; empty roots surface as `DirectoryMissing` (v1 semantics).
fn resolve_file(
    conn: &Connection,
    ws: &workspaces::WorkspaceEntry,
    file_path: &str,
) -> Result<PathBuf, CrossWorkspaceError> {
    let roots = compat_roots(conn, ws)?;
    if roots.is_empty() {
        return Err(CrossWorkspaceError::DirectoryMissing(ws.root_path.clone()));
    }
    roots
        .iter()
        .map(|r| r.join(file_path))
        .find(|p| p.is_file())
        .ok_or_else(|| CrossWorkspaceError::FileNotFound(file_path.to_string()))
}

/// Recursively searches files in a workspace root by file name (and optionally
/// content). Limits depth and result count to stay performant.
fn search_in_workspace(
    root: &Path,
    query: &str,
    match_content: bool,
) -> Result<Vec<FileMatch>, CrossWorkspaceError> {
    let mut results = Vec::new();
    let query_lower = query.to_lowercase();
    search_recursive(
        root,
        root,
        &query_lower,
        match_content,
        &mut results,
        0,
        100,
    )?;
    Ok(results)
}

fn search_recursive(
    root: &Path,
    current: &Path,
    query_lower: &str,
    match_content: bool,
    results: &mut Vec<FileMatch>,
    depth: u32,
    max_results: usize,
) -> Result<(), CrossWorkspaceError> {
    if depth > 5 || results.len() >= max_results {
        return Ok(());
    }

    for entry in std::fs::read_dir(current)? {
        if results.len() >= max_results {
            return Ok(());
        }
        let entry = entry?;
        let path = entry.path();

        // Skip hidden directories and common ignore patterns.
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || name_str == "node_modules" || name_str == "target" {
            continue;
        }

        if path.is_file() {
            if name_str.to_lowercase().contains(query_lower) {
                let rel = path
                    .strip_prefix(root)
                    .ok()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| name_str.to_string());
                results.push(FileMatch {
                    relative_path: rel,
                    match_type: MatchType::FileName,
                });
            } else if match_content && is_text_file(&path) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if content.to_lowercase().contains(query_lower) {
                        let rel = path
                            .strip_prefix(root)
                            .ok()
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_else(|| name_str.to_string());
                        results.push(FileMatch {
                            relative_path: rel,
                            match_type: MatchType::FileContent,
                        });
                    }
                }
            }
        } else if path.is_dir() {
            search_recursive(
                root,
                &path,
                query_lower,
                match_content,
                results,
                depth + 1,
                max_results,
            )?;
        }
    }
    Ok(())
}

fn is_text_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some(
            "rs" | "ts"
                | "tsx"
                | "js"
                | "jsx"
                | "json"
                | "md"
                | "txt"
                | "toml"
                | "yaml"
                | "yml"
                | "sql"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::workspace_open_set::WorkspaceOpenSetService;
    use crate::services::workspace_registry::WorkspaceRegistry;
    use std::fs;

    fn setup() -> (
        CrossWorkspaceService,
        WorkspaceOpenSetService,
        WorkspaceRegistry,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let svc = CrossWorkspaceService::new(db_path.clone());
        let open_svc = WorkspaceOpenSetService::new(db_path.clone());
        let reg = WorkspaceRegistry::new(db_path);
        (svc, open_svc, reg, dir)
    }

    #[test]
    fn search_finds_files_by_name() {
        let (svc, open_svc, reg, dir) = setup();
        let ws = dir.path().join("project");
        fs::create_dir_all(&ws).unwrap();
        fs::write(ws.join("hello.txt"), "content").unwrap();
        let entry = reg.register(&ws).unwrap();
        open_svc.open(&entry.id).unwrap();

        let outcome = svc.search_all_open("hello", false).unwrap();
        assert_eq!(outcome.groups.len(), 1);
        assert!(!outcome.groups[0].matches.is_empty());
    }

    #[test]
    fn search_skips_missing_directories() {
        let (svc, open_svc, reg, dir) = setup();
        let ws = dir.path().join("project");
        fs::create_dir_all(&ws).unwrap();
        let entry = reg.register(&ws).unwrap();
        open_svc.open(&entry.id).unwrap();
        fs::remove_dir_all(&ws).unwrap();

        let outcome = svc.search_all_open("anything", false).unwrap();
        assert!(outcome.skipped_workspace_ids.contains(&entry.id));
    }

    #[test]
    fn create_file_reference_reads_content() {
        let (svc, _open_svc, reg, dir) = setup();
        let ws = dir.path().join("project");
        fs::create_dir_all(&ws).unwrap();
        fs::write(ws.join("config.json"), "{\"key\":\"value\"}").unwrap();
        let entry = reg.register(&ws).unwrap();

        let reference = svc.create_file_reference(&entry.id, "config.json").unwrap();
        assert_eq!(reference.content_snapshot, "{\"key\":\"value\"}");
    }

    #[test]
    fn compare_files_detects_identical() {
        let (svc, _open_svc, reg, dir) = setup();
        let ws_a = dir.path().join("a");
        let ws_b = dir.path().join("b");
        fs::create_dir_all(&ws_a).unwrap();
        fs::create_dir_all(&ws_b).unwrap();
        fs::write(ws_a.join("f.txt"), "same").unwrap();
        fs::write(ws_b.join("f.txt"), "same").unwrap();
        let ea = reg.register(&ws_a).unwrap();
        let eb = reg.register(&ws_b).unwrap();

        let diff = svc.compare_files(&ea.id, "f.txt", &eb.id, "f.txt").unwrap();
        assert!(diff.is_identical);
    }

    // --- ADR-0021 root_path 兼容读取（v2 uuid 占位工作区）---

    /// Seeds a v2 workspace: `root_path` holds the uuid placeholder (ADR-0021)
    /// and the real root only exists as a `work_directories` row.
    fn seed_v2_workspace(svc: &CrossWorkspaceService, dir_root: &Path) -> String {
        let conn = svc.conn().unwrap();
        let id = uuid::Uuid::now_v7().to_string();
        workspaces::insert(
            &conn,
            &workspaces::WorkspaceEntry {
                id: id.clone(),
                root_path: id.clone(),
                color_tag: "ink-blue".to_string(),
                created_at: crate::domain::now_ms(),
                is_active: false,
                is_pinned: false,
            },
        )
        .unwrap();
        crate::store::repos::work_directories::add(
            &conn,
            &id,
            &dir_root.to_string_lossy(),
            "v2dir",
        )
        .unwrap();
        id
    }

    #[test]
    fn v2_uuid_placeholder_workspace_is_searchable() {
        let (svc, open_svc, _reg, dir) = setup();
        let ws_dir = dir.path().join("v2proj");
        fs::create_dir_all(&ws_dir).unwrap();
        fs::write(ws_dir.join("hello.txt"), "content").unwrap();
        let id = seed_v2_workspace(&svc, &ws_dir);

        open_svc.open(&id).unwrap();
        let outcome = svc.search_all_open("hello", false).unwrap();
        assert_eq!(outcome.groups.len(), 1);
        assert_eq!(outcome.groups[0].workspace_id, id);
        assert!(!outcome.groups[0].matches.is_empty());
        assert!(outcome.skipped_workspace_ids.is_empty());
    }

    #[test]
    fn v2_reference_probes_directories_in_added_order() {
        let (svc, _open_svc, _reg, dir) = setup();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        fs::write(second.join("only-in-second.txt"), "hit").unwrap();
        let id = seed_v2_workspace(&svc, &first);
        {
            let conn = svc.conn().unwrap();
            crate::store::repos::work_directories::add(
                &conn,
                &id,
                &second.to_string_lossy(),
                "second",
            )
            .unwrap();
        }

        let reference = svc
            .create_file_reference(&id, "only-in-second.txt")
            .unwrap();
        assert_eq!(reference.content_snapshot, "hit");
    }

    #[test]
    fn v2_compare_files_across_placeholder_roots() {
        let (svc, _open_svc, _reg, dir) = setup();
        let ws_a = dir.path().join("va");
        let ws_b = dir.path().join("vb");
        fs::create_dir_all(&ws_a).unwrap();
        fs::create_dir_all(&ws_b).unwrap();
        fs::write(ws_a.join("f.txt"), "same").unwrap();
        fs::write(ws_b.join("f.txt"), "same").unwrap();
        let ia = seed_v2_workspace(&svc, &ws_a);
        let ib = seed_v2_workspace(&svc, &ws_b);

        let diff = svc.compare_files(&ia, "f.txt", &ib, "f.txt").unwrap();
        assert!(diff.is_identical);
    }

    #[test]
    fn v2_workspace_with_stale_directory_is_skipped() {
        let (svc, open_svc, _reg, dir) = setup();
        // work_directories 行指向不存在的路径（repo 哑层不校验存在性），
        // root_path 又是 uuid 占位 → 无可达根目录。
        let ghost = dir.path().join("ghost");
        let id = seed_v2_workspace(&svc, &ghost);

        assert!(matches!(
            open_svc.open(&id),
            Err(crate::services::OpenSetError::DirectoryMissing(_))
        ));
        // 工作区不在 open 集合，搜索天然不触及；直接引用报 DirectoryMissing。
        assert!(matches!(
            svc.create_file_reference(&id, "any.txt"),
            Err(CrossWorkspaceError::DirectoryMissing(_))
        ));
    }
}
