//! GitService (SPEC ui-m1 D5 / AC5): git access through a whitelisted
//! subcommand set. Every invocation is `tokio::process::Command::new("git")`
//! with an argument array — never a shell string — executed in `repo_root`,
//! so metacharacters in filenames/messages are passed through verbatim.

use std::path::PathBuf;

use serde::Serialize;
use thiserror::Error;

/// The only subcommands this service will ever spawn.
const ALLOWED: [&str; 8] = [
    "status", "log", "branch", "worktree", "add", "commit", "push", "diff",
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
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
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
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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

    pub fn repo_root(&self) -> &std::path::Path {
        &self.repo_root
    }
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
}
