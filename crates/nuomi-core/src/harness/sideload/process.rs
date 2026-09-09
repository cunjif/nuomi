//! Plugin child process: argv-array spawn, stderr capture, kill.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;

use super::super::HarnessError;

/// How many stderr lines are kept per plugin for the boot report.
const STDERR_TAIL: usize = 20;

/// Last N stderr lines of a plugin process (boot-report material).
pub type StderrTail = Arc<Mutex<VecDeque<String>>>;

fn spawn_error(entry: &[PathBuf], message: impl Into<String>) -> HarnessError {
    HarnessError::PluginFailed {
        plugin: format!(
            "sideload:{}",
            entry.first().and_then(|p| p.to_str()).unwrap_or("?")
        ),
        phase: "spawn",
        message: message.into(),
    }
}

/// A spawned plugin process. Holds the child so it dies with the plugin
/// object; the stdio halves are handed to [`super::protocol::NppConnection`].
pub struct PluginProcess {
    child: tokio::sync::Mutex<Child>,
    stderr_tail: StderrTail,
}

impl PluginProcess {
    /// Spawns `argv` with `cwd` as working directory. Relative argv paths must
    /// already be resolved (`PluginManifest::resolved_entry`). stderr is piped
    /// and tailed; stdout/stdin go to the NPP connection.
    pub fn spawn(argv: &[PathBuf], cwd: &Path) -> Result<(Self, ChildStdio), HarnessError> {
        use std::process::Stdio;
        let (program, args) = argv
            .split_first()
            .ok_or_else(|| spawn_error(argv, "empty argv"))?;
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|e| spawn_error(argv, e.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| spawn_error(argv, "no stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| spawn_error(argv, "no stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| spawn_error(argv, "no stderr"))?;
        let stderr_tail: StderrTail = Arc::new(Mutex::new(VecDeque::new()));
        spawn_stderr_tap(stderr, Arc::clone(&stderr_tail));
        Ok((
            Self {
                child: tokio::sync::Mutex::new(child),
                stderr_tail: Arc::clone(&stderr_tail),
            },
            ChildStdio { stdin, stdout },
        ))
    }

    pub fn stderr_tail(&self) -> StderrTail {
        Arc::clone(&self.stderr_tail)
    }

    /// Reports whether the process has already exited (e.g. crashed mid-boot).
    pub async fn has_exited(&self) -> bool {
        self.child.lock().await.try_wait().ok().flatten().is_some()
    }

    /// Kills the process. Safe to call twice.
    pub async fn kill(&self) {
        let mut child = self.child.lock().await;
        let _ = child.kill().await;
    }
}

/// Piped stdio halves handed to the NPP connection.
pub struct ChildStdio {
    pub stdin: tokio::process::ChildStdin,
    pub stdout: tokio::process::ChildStdout,
}

// Manual impls: unwrap_err() in tests needs Debug, but the tokio stream halves
// don't warrant exposing their internals in test output.
impl std::fmt::Debug for PluginProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginProcess").finish()
    }
}

impl std::fmt::Debug for ChildStdio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildStdio").finish()
    }
}

fn spawn_stderr_tap(stderr: tokio::process::ChildStderr, tail: StderrTail) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(target: "nuomi::plugin::stderr", "{line}");
            let mut tail = tail
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if tail.len() == STDERR_TAIL {
                tail.pop_front();
            }
            tail.push_back(line);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(entries: &[&str]) -> Vec<PathBuf> {
        entries.iter().map(PathBuf::from).collect()
    }

    #[tokio::test]
    async fn spawns_and_kills() {
        let (proc, _stdio) = PluginProcess::spawn(
            &argv(&["node", "-e", "setInterval(()=>{},1e3)"]),
            Path::new("."),
        )
        .unwrap();
        assert!(!proc.has_exited().await);
        proc.kill().await;
        // Give the kill a moment to land, then the process must be gone.
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert!(proc.has_exited().await);
    }

    #[tokio::test]
    async fn spawn_failure_is_reported_not_panicked() {
        let err =
            PluginProcess::spawn(&argv(&["definitely-not-a-real-binary-xyz"]), Path::new("."))
                .unwrap_err();
        assert!(err.to_string().contains("during spawn"), "{err}");
    }

    #[tokio::test]
    async fn empty_argv_is_rejected() {
        assert!(PluginProcess::spawn(&[], Path::new(".")).is_err());
    }

    #[tokio::test]
    async fn stderr_lines_are_tailed() {
        let script =
            r#"console.error("boom-one"); console.error("boom-two"); setTimeout(()=>{},1e3)"#;
        let (proc, _stdio) =
            PluginProcess::spawn(&argv(&["node", "-e", script]), Path::new(".")).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        let tail: Vec<String> = proc
            .stderr_tail()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .cloned()
            .collect();
        assert!(tail.iter().any(|l| l.contains("boom-two")), "{tail:?}");
        proc.kill().await;
    }
}
