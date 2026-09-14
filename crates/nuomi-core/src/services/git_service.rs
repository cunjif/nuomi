//! GitService (SPEC ui-m1 D5 / AC5): git access through a whitelisted
//! subcommand set. Every invocation is `tokio::process::Command::new("git")`
//! with an argument array — never a shell string — executed in `repo_root`,
//! so metacharacters in filenames/messages are passed through verbatim.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The only subcommands this service will ever spawn. `merge` and `rev-parse`
/// back the two-phase worktree merge below; the rest serve the UI surfaces.
const ALLOWED: [&str; 10] = [
    "status",
    "log",
    "branch",
    "worktree",
    "add",
    "commit",
    "push",
    "diff",
    "merge",
    "rev-parse",
];

#[derive(Debug, Error)]
pub enum GitError {
    #[error("'{0}' is not in the git subcommand allowlist")]
    SubcommandNotAllowed(String),
    #[error("not a git repository: {0}")]
    NotARepository(String),
    #[error("git {cmd} failed: {stderr}")]
    CommandFailed { cmd: String, stderr: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Errors of the worktree isolation / two-phase merge flow. Kept separate
/// from [`GitError`] so the existing enum (exhaustively matched downstream,
/// e.g. by the Tauri IPC error mapping) stays stable.
#[derive(Debug, Error)]
pub enum WorktreeMergeError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("invalid worktree name: {0}")]
    InvalidWorktreeName(String),
    #[error("merge intent (de)serialization error: {0}")]
    IntentJson(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// One porcelain status line (simplified: rename arrows kept raw).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusEntry {
    pub index_status: char,
    pub worktree_status: char,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitInfo {
    pub hash: String,
    pub author: String,
    pub subject: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeInfo {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
}

/// Whitelisted git facade bound to one repository root.
#[derive(Debug, Clone)]
pub struct GitService {
    repo_root: PathBuf,
}

impl GitService {
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
        }
    }

    /// Runs one whitelisted git invocation and returns stdout.
    /// Public so callers can execute any allowlisted combination safely.
    pub async fn exec(&self, args: &[&str]) -> Result<String, GitError> {
        let (success, stdout, stderr) = self.exec_raw(args).await?;
        if !success {
            if stderr.to_ascii_lowercase().contains("not a git repository") {
                return Err(GitError::NotARepository(
                    self.repo_root.to_string_lossy().into_owned(),
                ));
            }
            return Err(GitError::CommandFailed {
                cmd: args.join(" "),
                stderr,
            });
        }
        Ok(stdout)
    }

    /// Like [`exec`] but returns `(success, stdout, stderr)` verbatim instead
    /// of mapping failure to an error. Needed by the merge flow, where a
    /// failed command is an *expected* outcome (conflicts) whose stdout
    /// carries the conflicted-file list.
    async fn exec_raw(&self, args: &[&str]) -> Result<(bool, String, String), GitError> {
        let subcommand = args
            .first()
            .ok_or_else(|| GitError::SubcommandNotAllowed("<empty>".into()))?;
        if !ALLOWED.contains(subcommand) {
            return Err(GitError::SubcommandNotAllowed((*subcommand).to_string()));
        }
        // Argument array + explicit current_dir; no shell involved anywhere.
        let output = tokio::process::Command::new("git")
            .args(args)
            .current_dir(&self.repo_root)
            .output()
            .await?;
        Ok((
            output.status.success(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }

    /// Short working-tree status (`git status --porcelain`).
    pub async fn status(&self) -> Result<Vec<StatusEntry>, GitError> {
        let out = self.exec(&["status", "--porcelain"]).await?;
        Ok(out
            .lines()
            .filter(|l| l.len() >= 3)
            .map(|line| StatusEntry {
                index_status: line.as_bytes()[0] as char,
                worktree_status: line.as_bytes()[1] as char,
                path: line[3..].trim().to_string(),
            })
            .collect())
    }

    /// Recent commits, newest first.
    pub async fn log(&self, limit: u32) -> Result<Vec<CommitInfo>, GitError> {
        let out = self
            .exec(&[
                "log",
                &format!("-n{limit}"),
                "--pretty=format:%H%x09%an%x09%s",
            ])
            .await?;
        Ok(out
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|line| {
                let mut parts = line.splitn(3, '\t');
                CommitInfo {
                    hash: parts.next().unwrap_or_default().to_string(),
                    author: parts.next().unwrap_or_default().to_string(),
                    subject: parts.next().unwrap_or_default().to_string(),
                }
            })
            .collect())
    }

    /// Linked worktrees (`git worktree list --porcelain`).
    pub async fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>, GitError> {
        let out = self.exec(&["worktree", "list", "--porcelain"]).await?;
        let mut result = Vec::new();
        let mut current: Option<WorktreeInfo> = None;
        for line in out.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                if let Some(c) = current.take() {
                    result.push(c);
                }
                current = Some(WorktreeInfo {
                    path: path.to_string(),
                    head: None,
                    branch: None,
                });
            } else if let Some(head) = line.strip_prefix("HEAD ") {
                if let Some(c) = current.as_mut() {
                    c.head = Some(head.to_string());
                }
            } else if let Some(branch) = line.strip_prefix("branch ") {
                if let Some(c) = current.as_mut() {
                    c.branch = Some(branch.trim_start_matches("refs/heads/").to_string());
                }
            }
            // "detached" and blank lines carry no extra fields we surface
        }
        if let Some(c) = current.take() {
            result.push(c);
        }
        Ok(result)
    }

    /// Stages paths (`git add -- <paths>`; `--` blocks option injection).
    pub async fn stage(&self, paths: &[&str]) -> Result<(), GitError> {
        let mut args: Vec<&str> = vec!["add", "--"];
        args.extend_from_slice(paths);
        self.exec(&args).await.map(|_| ())
    }

    /// Commits the staged tree with `-m` (message never touches a shell).
    pub async fn commit(&self, message: &str) -> Result<String, GitError> {
        self.exec(&["commit", "-m", message]).await
    }

    pub async fn push(&self, remote: &str, branch: &str) -> Result<String, GitError> {
        self.exec(&["push", remote, branch]).await
    }

    /// Unstaged diff of tracked files.
    pub async fn diff(&self) -> Result<String, GitError> {
        self.exec(&["diff"]).await
    }

    /// Diff for a specific path. `staged` selects `--cached` (index vs HEAD)
    /// vs the default working-tree-vs-index diff. `--` blocks option injection.
    pub async fn diff_for_path(&self, path: &str, staged: bool) -> Result<String, GitError> {
        if staged {
            self.exec(&["diff", "--cached", "--", path]).await
        } else {
            self.exec(&["diff", "--", path]).await
        }
    }

    /// Diff of a file git does not track yet. `git diff` reports nothing for
    /// untracked paths, so compare against `/dev/null`; that form exits 1
    /// whenever it finds differences, which is the expected outcome here.
    pub async fn diff_untracked(&self, path: &str) -> Result<String, GitError> {
        let (success, stdout, stderr) = self
            .exec_raw(&["diff", "--no-index", "--", "/dev/null", path])
            .await?;
        if !success && stdout.trim().is_empty() {
            return Err(GitError::CommandFailed {
                cmd: format!("diff --no-index -- /dev/null {path}"),
                stderr,
            });
        }
        Ok(stdout)
    }

    pub fn repo_root(&self) -> &std::path::Path {
        &self.repo_root
    }
}

// ---------------------------------------------------------------------------
// Worktree isolation + two-phase merge (P1; codeg work_task/git.rs Stage A/B
// + parallel-code worktree isolation).
//
// Invariants:
// - Stage A merges the base INTO the task branch inside the worktree; any
//   conflict stays inside the worktree and can never reach the main workspace.
// - Stage B merges the task branch back with `--no-ff`; the merge intent is
//   persisted to a JSON sidecar BEFORE the side effect, so a crash leaves a
//   recoverable trail. On failure inside the main workspace the merge is
//   best-effort aborted to keep the main tree clean.
// - Recovery (`recover_merges`) consults git truth only: a commit already
//   reachable from another branch means the merge landed → cleanup; otherwise
//   the item is reported as pending replay, never auto-retried.
// ---------------------------------------------------------------------------

/// Directory inside the repo root holding all task worktrees.
pub const WORKTREES_DIR: &str = ".worktrees";
/// Branch namespace prefix for task worktrees.
pub const BRANCH_PREFIX: &str = "nuomi/";
/// Directory inside the repo root holding merge-intent sidecars.
pub const NUOMI_META_DIR: &str = ".nuomi";

/// Persisted merge intent — one JSON sidecar per task at
/// `.nuomi/merge-intent-<task>.json`, written atomically (temp + rename)
/// before any merge side effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeIntent {
    pub task: String,
    /// HEAD of the base branch at intent time (Stage B baseline).
    pub base_commit: String,
    /// Tip of the task branch being merged.
    pub head_commit: String,
    /// `"stage-a"` or `"stage-b"`.
    pub stage: String,
    /// Unix seconds; `SystemTime` avoids a time-crate feature dependency.
    pub timestamp_unix_secs: u64,
}

/// Outcome of Stage A (base merged into the task branch inside the worktree).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "outcome")]
pub enum StageAOutcome {
    Merged,
    AlreadyUpToDate,
    /// Conflicted files, left in place inside the worktree on purpose.
    Conflict {
        files: Vec<String>,
    },
}

/// Stage B result: main-workspace HEAD before/after the `--no-ff` merge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StageBResult {
    pub head_before: String,
    pub head_after: String,
}

/// One recovery finding: sidecar intent plus whether git says it already
/// landed. `merged == false` means pending replay (reported, not retried).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryItem {
    pub task: String,
    pub intent: MergeIntent,
    pub merged: bool,
}

/// Validates a worktree name: a single path component, no separators,
/// drive letters, whitespace or dot-leading names (keeps branch and sidecar
/// filenames safe on every platform).
fn validate_worktree_name(name: &str) -> Result<(), WorktreeMergeError> {
    let ok = !name.is_empty()
        && !name.starts_with('.')
        && !name
            .chars()
            .any(|c| matches!(c, '/' | '\\' | ':' | ' ' | '\t') || c.is_control());
    if ok {
        Ok(())
    } else {
        Err(WorktreeMergeError::InvalidWorktreeName(name.to_string()))
    }
}

fn worktree_branch(name: &str) -> String {
    format!("{BRANCH_PREFIX}{name}")
}

/// Path of the worktree checkout, joined via `PathBuf` (no hardcoded
/// separators — correct on Windows and POSIX alike).
pub fn worktree_path(repo: &Path, name: &str) -> PathBuf {
    repo.join(WORKTREES_DIR).join(name)
}

fn sidecar_path(repo: &Path, name: &str) -> PathBuf {
    repo.join(NUOMI_META_DIR)
        .join(format!("merge-intent-{name}.json"))
}

fn now_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Atomic sidecar write: temp file + rename (pre-remove on Windows where
/// rename cannot overwrite an existing target), mirroring
/// `WorkspaceService::write_file_atomic`.
fn write_intent(
    repo: &Path,
    task: &str,
    stage: &str,
    base_commit: &str,
    head_commit: &str,
) -> Result<(), WorktreeMergeError> {
    let intent = MergeIntent {
        task: task.to_string(),
        base_commit: base_commit.to_string(),
        head_commit: head_commit.to_string(),
        stage: stage.to_string(),
        timestamp_unix_secs: now_unix_secs(),
    };
    let path = sidecar_path(repo, task);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&intent)?;
    let tmp = path.with_extension(format!("{}tmp", uuid::Uuid::now_v7().simple()));
    std::fs::write(&tmp, json)?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    match std::fs::rename(&tmp, &path) {
        Ok(()) => Ok(()),
        Err(e) => {
            // best effort cleanup of the orphaned temp file
            let _ = std::fs::remove_file(&tmp);
            Err(e.into())
        }
    }
}

fn remove_sidecar(repo: &Path, name: &str) {
    let _ = std::fs::remove_file(sidecar_path(repo, name));
}

/// Creates a linked worktree at `.worktrees/<name>` on a new branch
/// `nuomi/<name>` rooted at `base`. Returns the worktree directory.
pub async fn create_worktree(
    repo: &Path,
    name: &str,
    base: &str,
) -> Result<PathBuf, WorktreeMergeError> {
    validate_worktree_name(name)?;
    let wt_arg = Path::new(WORKTREES_DIR).join(name);
    std::fs::create_dir_all(repo.join(WORKTREES_DIR))?;
    let svc = GitService::new(repo);
    svc.exec(&[
        "worktree",
        "add",
        &wt_arg.to_string_lossy(),
        "-b",
        &worktree_branch(name),
        base,
    ])
    .await?;
    Ok(worktree_path(repo, name))
}

/// Stage A: inside the worktree, merge `base` into the task branch.
/// Conflicts are left in the worktree (sidecar kept → recovery reports
/// pending replay); the main workspace is never touched. Any other failure
/// also keeps the sidecar so the interrupted merge stays visible.
pub async fn merge_base_into_worktree(
    repo: &Path,
    name: &str,
    base: &str,
) -> Result<StageAOutcome, WorktreeMergeError> {
    validate_worktree_name(name)?;
    let wt_dir = worktree_path(repo, name);
    if !wt_dir.is_dir() {
        return Err(GitError::NotARepository(wt_dir.to_string_lossy().into_owned()).into());
    }
    let svc = GitService::new(&wt_dir);
    let head_commit = svc.exec(&["rev-parse", "HEAD"]).await?.trim().to_string();
    let base_commit = svc.exec(&["rev-parse", base]).await?.trim().to_string();
    write_intent(repo, name, "stage-a", &base_commit, &head_commit)?;

    let (success, stdout, stderr) = svc.exec_raw(&["merge", base]).await?;
    if success {
        if stdout.contains("Already up to date") {
            remove_sidecar(repo, name);
            Ok(StageAOutcome::AlreadyUpToDate)
        } else {
            remove_sidecar(repo, name);
            Ok(StageAOutcome::Merged)
        }
    } else if stdout.contains("CONFLICT") {
        // Conflict markers stay inside the worktree by design.
        let files = conflict_files(&stdout);
        Ok(StageAOutcome::Conflict { files })
    } else {
        Err(GitError::CommandFailed {
            cmd: format!("merge {base}"),
            stderr,
        }
        .into())
    }
}

/// Extracts conflicted paths from `git merge` stdout lines of the form
/// `CONFLICT (content): Merge conflict in <path>`.
fn conflict_files(merge_stdout: &str) -> Vec<String> {
    merge_stdout
        .lines()
        .filter(|l| l.starts_with("CONFLICT"))
        .filter_map(|l| l.split_once("Merge conflict in ").map(|(_, f)| f.trim()))
        .map(str::to_string)
        .collect()
}

/// Stage B: in the main workspace, `git merge --no-ff nuomi/<name>`, with
/// HEAD recorded before/after. The intent sidecar is persisted BEFORE the
/// merge and removed after success. On failure the merge is best-effort
/// aborted (main tree stays clean) and the sidecar kept for recovery.
pub async fn merge_worktree_into_base(
    repo: &Path,
    name: &str,
) -> Result<StageBResult, WorktreeMergeError> {
    validate_worktree_name(name)?;
    let branch = worktree_branch(name);
    let svc = GitService::new(repo);
    let head_before = svc.exec(&["rev-parse", "HEAD"]).await?.trim().to_string();
    let head_commit = svc.exec(&["rev-parse", &branch]).await?.trim().to_string();
    write_intent(repo, name, "stage-b", &head_before, &head_commit)?;

    let (success, _stdout, stderr) = svc.exec_raw(&["merge", "--no-ff", &branch]).await?;
    if !success {
        // Keep the main workspace pristine; recovery reports the pending merge.
        let _ = svc.exec_raw(&["merge", "--abort"]).await;
        return Err(GitError::CommandFailed {
            cmd: format!("merge --no-ff {branch}"),
            stderr,
        }
        .into());
    }
    let head_after = svc.exec(&["rev-parse", "HEAD"]).await?.trim().to_string();
    remove_sidecar(repo, name);
    Ok(StageBResult {
        head_before,
        head_after,
    })
}

/// Scans `.nuomi/merge-intent-*.json` sidecars and decides each one from git
/// truth: if `head_commit` is reachable from any branch other than the task
/// branch, the merge landed → sidecar removed and worktree+branch cleaned up;
/// otherwise reported as pending replay. Never retries a merge automatically.
pub async fn recover_merges(repo: &Path) -> Result<Vec<RecoveryItem>, WorktreeMergeError> {
    let meta = repo.join(NUOMI_META_DIR);
    let mut items = Vec::new();
    if !meta.is_dir() {
        return Ok(items);
    }
    let mut sidecars: Vec<PathBuf> = Vec::new();
    for entry in std::fs::read_dir(&meta)? {
        let path = entry?.path();
        let is_sidecar = path.file_name().is_some_and(|f| {
            let f = f.to_string_lossy();
            f.starts_with("merge-intent-") && f.ends_with(".json")
        });
        if is_sidecar {
            sidecars.push(path);
        }
    }
    sidecars.sort();

    let svc = GitService::new(repo);
    for path in sidecars {
        let intent: MergeIntent = match serde_json::from_str(&std::fs::read_to_string(&path)?) {
            Ok(intent) => intent,
            Err(e) => {
                // A corrupt sidecar must not stall recovery of the others.
                tracing::warn!(path = %path.display(), error = %e, "skipping unreadable merge intent");
                continue;
            }
        };
        // git truth: which local branches contain the task tip? A failed
        // lookup (unknown object) conservatively counts as "not merged".
        // Output line markers: '*' = current branch, '+' = checked out in
        // another worktree, plain spaces otherwise.
        let merged = match svc
            .exec(&["branch", "--contains", &intent.head_commit])
            .await
        {
            Ok(out) => out.lines().any(|l| {
                let b = l.trim_start_matches(['*', '+', ' ']).trim();
                !b.is_empty() && b != worktree_branch(&intent.task)
            }),
            Err(_) => false,
        };
        if merged {
            remove_sidecar(repo, &intent.task);
            let _ = remove_worktree(repo, &intent.task).await;
        }
        items.push(RecoveryItem {
            task: intent.task.clone(),
            intent,
            merged,
        });
    }
    Ok(items)
}

/// Removes the worktree (`--force`), deletes the `nuomi/<name>` branch and
/// clears the sidecar. Idempotent: steps that find nothing to remove are
/// logged and skipped rather than failing (safe for recovery paths).
pub async fn remove_worktree(repo: &Path, name: &str) -> Result<(), WorktreeMergeError> {
    validate_worktree_name(name)?;
    let svc = GitService::new(repo);
    let wt_arg = Path::new(WORKTREES_DIR).join(name);
    if let Err(e) = svc
        .exec(&["worktree", "remove", "--force", &wt_arg.to_string_lossy()])
        .await
    {
        tracing::warn!(name, error = %e, "worktree remove failed (already gone?)");
    }
    if let Err(e) = svc.exec(&["branch", "-D", &worktree_branch(name)]).await {
        tracing::warn!(name, error = %e, "branch delete failed (already gone?)");
    }
    remove_sidecar(repo, name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// True when a `git` executable is reachable on PATH.
    fn git_available() -> bool {
        std::process::Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Creates a real repo in a tempdir with a local identity.
    async fn init_repo() -> (tempfile::TempDir, GitService) {
        let dir = tempfile::tempdir().unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec!["config", "user.email", "test@nuomi.local"],
            vec!["config", "user.name", "nuomi test"],
        ] {
            let out = std::process::Command::new("git")
                .args(&args)
                .current_dir(dir.path())
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?} failed");
        }
        let svc = GitService::new(dir.path());
        (dir, svc)
    }

    async fn commit_file(svc: &GitService, name: &str, content: &str, msg: &str) {
        let path = svc.repo_root().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
        svc.stage(&[name]).await.unwrap();
        svc.commit(msg).await.unwrap();
    }

    #[tokio::test]
    async fn stage_commit_log_roundtrip_on_real_repo() {
        if !git_available() {
            eprintln!("git not on PATH — skipping real-repo test");
            return;
        }
        let (_dir, svc) = init_repo().await;
        commit_file(&svc, "README.md", "# nuomi\n", "initial commit").await;
        commit_file(&svc, "src/main.rs", "fn main() {}\n", "add main").await;

        let log = svc.log(10).await.unwrap();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].subject, "add main");
        assert_eq!(log[1].subject, "initial commit");
        assert!(!log[0].hash.is_empty());
        assert_eq!(log[1].author, "nuomi test");

        // clean tree after commits
        assert!(svc.status().await.unwrap().is_empty());

        // limited log
        assert_eq!(svc.log(1).await.unwrap().len(), 1);

        // worktree listing contains the main checkout
        let wts = svc.list_worktrees().await.unwrap();
        assert_eq!(wts.len(), 1);
        assert_eq!(wts[0].branch.as_deref(), Some("main"));
        assert!(wts[0].head.is_some());
    }

    #[tokio::test]
    async fn status_detects_untracked_and_diff_shows_changes() {
        if !git_available() {
            eprintln!("git not on PATH — skipping real-repo test");
            return;
        }
        let (_dir, svc) = init_repo().await;
        commit_file(&svc, "a.txt", "one\n", "first").await;

        std::fs::write(svc.repo_root().join("b.txt"), "untracked\n").unwrap();
        let entries = svc.status().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "b.txt");
        assert_eq!(entries[0].index_status, '?');

        std::fs::write(svc.repo_root().join("a.txt"), "one\ntwo\n").unwrap();
        let diff = svc.diff().await.unwrap();
        assert!(diff.contains("+two"));
    }

    #[tokio::test]
    async fn non_allowlisted_subcommand_is_rejected_before_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new(dir.path());
        for bad in ["rebase", "reset --hard", "rm -rf /"] {
            let err = svc.exec(&[bad]).await.unwrap_err();
            assert!(
                matches!(err, GitError::SubcommandNotAllowed(ref s) if s.starts_with(bad.split(' ').next().unwrap())),
                "{bad} → {err}"
            );
        }
        // even inside the repo, rebase stays rejected
        let (_rdir, rsvc) = init_repo().await;
        assert!(matches!(
            rsvc.exec(&["rebase"]).await,
            Err(GitError::SubcommandNotAllowed(_))
        ));
    }

    #[tokio::test]
    async fn hostile_filenames_pass_through_safely_as_arg_array() {
        if !git_available() {
            eprintln!("git not on PATH — skipping real-repo test");
            return;
        }
        let (_dir, svc) = init_repo().await;
        // Injection-shaped name: spaces, semicolons, $(), backticks, quotes.
        // (Double quotes are excluded because NTFS itself forbids them.)
        let hostile = "a; rm -rf ~ ;b $(echo pwned) `x` 's'.txt";
        commit_file(&svc, hostile, "content\n", "hostile filename").await;

        // The file was staged and committed as ONE literal path — no shell ran.
        let log = svc.log(1).await.unwrap();
        assert_eq!(log[0].subject, "hostile filename");
        assert!(svc.status().await.unwrap().is_empty());
        assert!(svc.repo_root().join(hostile).exists());
    }

    #[tokio::test]
    async fn non_git_directory_reports_not_a_repository() {
        if !git_available() {
            eprintln!("git not on PATH — skipping real-repo test");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let svc = GitService::new(dir.path());
        let err = svc.status().await.unwrap_err();
        assert!(matches!(err, GitError::NotARepository(_)), "{err}");
        assert!(matches!(svc.log(5).await, Err(GitError::NotARepository(_))));
    }

    #[tokio::test]
    async fn failing_whitelisted_command_maps_to_command_failed() {
        if !git_available() {
            eprintln!("git not on PATH — skipping real-repo test");
            return;
        }
        let (_dir, svc) = init_repo().await;
        // push without remote fails inside git itself
        let err = svc.push("origin", "main").await.unwrap_err();
        assert!(matches!(err, GitError::CommandFailed { .. }), "{err}");
    }

    // -----------------------------------------------------------------------
    // Worktree isolation + two-phase merge
    // -----------------------------------------------------------------------

    /// Writes a fake crash-leftover sidecar for `task`.
    fn write_sidecar(repo: &Path, task: &str, head_commit: &str, stage: &str) -> PathBuf {
        let intent = MergeIntent {
            task: task.to_string(),
            base_commit: "0000000000000000000000000000000000000000".to_string(),
            head_commit: head_commit.to_string(),
            stage: stage.to_string(),
            timestamp_unix_secs: 0,
        };
        let path = sidecar_path(repo, task);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_string(&intent).unwrap()).unwrap();
        path
    }

    #[tokio::test]
    async fn worktree_two_phase_merge_full_flow() {
        if !git_available() {
            eprintln!("git not on PATH — skipping real-repo test");
            return;
        }
        let (dir, svc) = init_repo().await;
        let repo = dir.path().to_path_buf();
        commit_file(&svc, "shared.txt", "base\n", "base commit").await;

        // create → the worktree mirrors the base content
        let wt = create_worktree(&repo, "task-1", "main").await.unwrap();
        assert!(wt.is_dir());
        assert_eq!(
            std::fs::read_to_string(wt.join("shared.txt")).unwrap(),
            "base\n"
        );

        // commit inside the worktree on branch nuomi/task-1
        let wsvc = GitService::new(&wt);
        std::fs::write(wt.join("task.txt"), "from task\n").unwrap();
        wsvc.stage(&["task.txt"]).await.unwrap();
        wsvc.commit("task change").await.unwrap();
        let task_tip = wsvc.exec(&["rev-parse", "HEAD"]).await.unwrap();
        let task_tip = task_tip.trim();

        // main moves on after the worktree was cut
        commit_file(&svc, "main.txt", "main\n", "main change").await;

        // Stage A: base merged into the task branch, conflict-free
        let outcome = merge_base_into_worktree(&repo, "task-1", "main")
            .await
            .unwrap();
        assert_eq!(outcome, StageAOutcome::Merged);
        assert!(wt.join("main.txt").exists());
        // isolation: the task file never leaked into the main workspace
        assert!(!repo.join("task.txt").exists());
        // Stage A sidecar cleared on success
        assert!(!sidecar_path(&repo, "task-1").exists());

        // Stage B: --no-ff merge back into main, HEAD recorded both sides
        let result = merge_worktree_into_base(&repo, "task-1").await.unwrap();
        assert_ne!(result.head_before, result.head_after);
        assert!(!sidecar_path(&repo, "task-1").exists());

        // git truth: HEAD (main) now contains the task commit
        let head_now = svc.exec(&["rev-parse", "HEAD"]).await.unwrap();
        assert_eq!(head_now.trim(), result.head_after);
        let contains = svc.exec(&["branch", "--contains", task_tip]).await.unwrap();
        assert!(contains
            .lines()
            .any(|l| l.trim_start_matches(['*', '+', ' ']).trim() == "main"));

        // crash recovery: a leftover intent whose commit already landed is
        // cleaned up (sidecar removed, worktree + branch deleted)
        let sidecar = write_sidecar(&repo, "task-1", task_tip, "stage-b");
        let report = recover_merges(&repo).await.unwrap();
        assert_eq!(report.len(), 1);
        assert!(report[0].merged);
        assert_eq!(report[0].task, "task-1");
        assert!(!sidecar.exists());
        assert!(!wt.exists());
        let branches = svc.exec(&["branch", "--list"]).await.unwrap();
        assert!(!branches.contains("nuomi/task-1"));

        // recovery on a repo without sidecars reports nothing
        assert!(recover_merges(&repo).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn recover_reports_pending_when_commit_not_merged() {
        if !git_available() {
            eprintln!("git not on PATH — skipping real-repo test");
            return;
        }
        let (dir, svc) = init_repo().await;
        let repo = dir.path().to_path_buf();
        commit_file(&svc, "shared.txt", "base\n", "base commit").await;

        // a task branch whose tip never reached main
        let wt = create_worktree(&repo, "stuck", "main").await.unwrap();
        let wsvc = GitService::new(&wt);
        std::fs::write(wt.join("wip.txt"), "wip\n").unwrap();
        wsvc.stage(&["wip.txt"]).await.unwrap();
        wsvc.commit("wip").await.unwrap();
        let tip = wsvc.exec(&["rev-parse", "HEAD"]).await.unwrap();
        let tip = tip.trim().to_string();

        let sidecar = write_sidecar(&repo, "stuck", &tip, "stage-b");
        let report = recover_merges(&repo).await.unwrap();
        assert_eq!(report.len(), 1);
        assert!(!report[0].merged);
        assert_eq!(report[0].intent.stage, "stage-b");
        // pending replay: nothing is auto-retried or cleaned
        assert!(sidecar.exists());
        assert!(wt.exists());
        assert!(svc
            .exec(&["branch", "--list"])
            .await
            .unwrap()
            .contains("nuomi/stuck"));
    }

    #[tokio::test]
    async fn stage_a_conflict_stays_inside_worktree() {
        if !git_available() {
            eprintln!("git not on PATH — skipping real-repo test");
            return;
        }
        let (dir, svc) = init_repo().await;
        let repo = dir.path().to_path_buf();
        commit_file(&svc, "f.txt", "base\n", "base").await;

        let wt = create_worktree(&repo, "conflict", "main").await.unwrap();

        // main and the worktree edit the same file differently
        commit_file(&svc, "f.txt", "main wins\n", "main edit").await;
        // baseline right before Stage A: main HEAD must not move on conflict
        let head_before = svc.exec(&["rev-parse", "HEAD"]).await.unwrap();
        let head_before = head_before.trim();
        let wsvc = GitService::new(&wt);
        std::fs::write(wt.join("f.txt"), "task wins\n").unwrap();
        wsvc.stage(&["f.txt"]).await.unwrap();
        wsvc.commit("task edit").await.unwrap();

        let outcome = merge_base_into_worktree(&repo, "conflict", "main")
            .await
            .unwrap();
        match outcome {
            StageAOutcome::Conflict { files } => {
                assert!(
                    files.iter().any(|f| f.contains("f.txt")),
                    "conflicted files: {files:?}"
                );
            }
            other => panic!("expected conflict, got {other:?}"),
        }
        // conflict markers live inside the worktree only
        let wt_content = std::fs::read_to_string(wt.join("f.txt")).unwrap();
        assert!(wt_content.contains("<<<<<<<"));

        // the main workspace is untouched: content, HEAD and merge state
        assert_eq!(
            std::fs::read_to_string(repo.join("f.txt")).unwrap(),
            "main wins\n"
        );
        let head_after_fail = svc.exec(&["rev-parse", "HEAD"]).await.unwrap();
        assert_eq!(head_after_fail.trim(), head_before);
        let status = svc.status().await.unwrap();
        assert!(
            status
                .iter()
                .all(|e| e.index_status != 'U' && e.worktree_status != 'U'),
            "main workspace has unmerged entries: {status:?}"
        );
    }

    #[tokio::test]
    async fn remove_worktree_cleans_dir_branch_and_sidecar() {
        if !git_available() {
            eprintln!("git not on PATH — skipping real-repo test");
            return;
        }
        let (dir, svc) = init_repo().await;
        let repo = dir.path().to_path_buf();
        commit_file(&svc, "shared.txt", "base\n", "base").await;
        let wt = create_worktree(&repo, "gone", "main").await.unwrap();
        let sidecar = write_sidecar(&repo, "gone", "0".repeat(40).as_str(), "stage-a");

        remove_worktree(&repo, "gone").await.unwrap();
        assert!(!wt.exists());
        assert!(!sidecar.exists());
        assert!(!svc
            .exec(&["branch", "--list"])
            .await
            .unwrap()
            .contains("nuomi/gone"));
        // idempotent: removing again still succeeds
        remove_worktree(&repo, "gone").await.unwrap();
    }

    #[tokio::test]
    async fn invalid_worktree_names_are_rejected_before_spawn() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["", "..", ".", ".hidden", "a/b", "a\\b", "a:b", "a b"] {
            let err = create_worktree(dir.path(), bad, "main").await.unwrap_err();
            assert!(
                matches!(err, WorktreeMergeError::InvalidWorktreeName(ref n) if n == bad),
                "{bad} → {err}"
            );
        }
    }
}
