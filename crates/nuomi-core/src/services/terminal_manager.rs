//! Terminal manager (ADR-0024 Part B): interactive PTY terminals for the
//! builtin Terminal area.
//!
//! Wraps [`portable_pty`] (Windows ConPTY / Unix PTY) to spawn an interactive
//! shell and pipe stdin/stdout. The reader thread pumps output bytes through
//! the [`TerminalEventEmitter`] trait so the shell layer can emit Tauri events
//! without nuomi-core depending on tauri.

use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use thiserror::Error;
use uuid::Uuid;

/// Default terminal dimensions (matches xterm.js default cols/rows).
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

/// Errors from the terminal manager.
#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("terminal not found: {0}")]
    NotFound(String),
    #[error("failed to open pty: {0}")]
    Open(String),
    #[error("failed to spawn shell: {0}")]
    Spawn(String),
    #[error("pty io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to resize pty: {0}")]
    Resize(String),
}

/// Abstraction over the event channel that carries terminal output to the
/// frontend. The shell layer implements this to emit Tauri events; nuomi-core
/// stays tauri-free.
pub trait TerminalEventEmitter: Send + Sync {
    /// Called by the reader thread for each chunk of output bytes.
    fn emit_output(&self, terminal_id: &str, data: &[u8]);
}

/// A live terminal handle: the master PTY (for resize) + the writer (for
/// stdin) + the child process + the reader thread join handle.
struct TerminalHandle {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn std::io::Write + Send>,
    child: Box<dyn portable_pty::Child + Send>,
    reader_thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for TerminalHandle {
    fn drop(&mut self) {
        // Best-effort kill + reap; the child may have already exited.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(handle) = self.reader_thread.take() {
            let _ = handle.join();
        }
    }
}

/// The terminal manager owns all live terminals by id. Thread-safe via
/// `Arc<Mutex<HashMap>>` — `create`/`write`/`resize`/`kill` all acquire the
/// lock briefly.
#[derive(Clone)]
pub struct TerminalManager {
    terminals: Arc<Mutex<HashMap<String, TerminalHandle>>>,
    emitter: Arc<dyn TerminalEventEmitter>,
}

impl TerminalManager {
    pub fn new(emitter: Arc<dyn TerminalEventEmitter>) -> Self {
        Self {
            terminals: Arc::new(Mutex::new(HashMap::new())),
            emitter,
        }
    }

    /// Spawns a new interactive shell in a PTY at the given cwd (optional).
    /// Returns the terminal id. The reader thread starts immediately and
    /// pumps output through the emitter.
    pub fn create(&self, cwd: Option<&Path>) -> Result<String, TerminalError> {
        let (program, args) = default_shell();
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: DEFAULT_ROWS,
                cols: DEFAULT_COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| TerminalError::Open(e.to_string()))?;

        let mut cmd = CommandBuilder::new(&program);
        if !args.is_empty() {
            cmd.args(args.iter().map(|a| a.as_str()));
        }
        if let Some(dir) = cwd {
            cmd.cwd(dir);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| TerminalError::Spawn(e.to_string()))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| TerminalError::Open(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| TerminalError::Open(e.to_string()))?;
        // Closing the slave handle lets the reader observe EOF on child exit.
        drop(pair.slave);

        let id = Uuid::now_v7().to_string();
        let emitter = self.emitter.clone();
        let emit_id = id.clone();
        let reader_thread = std::thread::Builder::new()
            .name("nuomi-terminal-reader".to_string())
            .spawn(move || {
                let mut reader = reader;
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => emitter.emit_output(&emit_id, &buf[..n]),
                    }
                }
            })
            .map_err(|e| TerminalError::Spawn(e.to_string()))?;

        let master = pair.master;
        let mut terminals = self.terminals.lock().expect("terminal mutex poisoned");
        terminals.insert(id.clone(), TerminalHandle {
            master,
            writer,
            child,
            reader_thread: Some(reader_thread),
        });
        Ok(id)
    }

    /// Writes data to the terminal's stdin (master PTY).
    pub fn write(&self, id: &str, data: &[u8]) -> Result<(), TerminalError> {
        let mut terminals = self.terminals.lock().expect("terminal mutex poisoned");
        let handle = terminals
            .get_mut(id)
            .ok_or_else(|| TerminalError::NotFound(id.to_string()))?;
        handle.writer.write_all(data)?;
        handle.writer.flush()?;
        Ok(())
    }

    /// Resizes the terminal's PTY.
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), TerminalError> {
        let mut terminals = self.terminals.lock().expect("terminal mutex poisoned");
        let handle = terminals
            .get_mut(id)
            .ok_or_else(|| TerminalError::NotFound(id.to_string()))?;
        handle
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| TerminalError::Resize(e.to_string()))?;
        Ok(())
    }

    /// Kills the terminal and removes it from the manager.
    pub fn kill(&self, id: &str) -> Result<(), TerminalError> {
        let mut terminals = self.terminals.lock().expect("terminal mutex poisoned");
        terminals
            .remove(id)
            .ok_or_else(|| TerminalError::NotFound(id.to_string()))?;
        // Drop runs kill + reap + join.
        Ok(())
    }

    /// Kills all terminals (e.g. on workspace close or app shutdown).
    pub fn kill_all(&self) {
        let mut terminals = self.terminals.lock().expect("terminal mutex poisoned");
        terminals.clear();
    }
}

/// Detects the default shell for the current platform.
/// Windows: PowerShell (pwsh.exe → powershell.exe → cmd.exe fallback).
/// Unix: $SHELL → /bin/bash fallback.
fn default_shell() -> (String, Vec<String>) {
    if cfg!(windows) {
        // Prefer pwsh (PowerShell 7+), then Windows PowerShell, then cmd.
        for candidate in ["pwsh.exe", "powershell.exe", "cmd.exe"] {
            if which_in_path(candidate).is_some() {
                return (candidate.to_string(), vec!["-NoLogo".to_string()]);
            }
        }
        ("cmd.exe".to_string(), vec![])
    } else {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        (shell, vec![])
    }
}

/// Minimal `which`-like check: looks for the program on PATH. Returns the
/// resolved path if found. Only used for the Windows shell fallback chain.
fn which_in_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A test emitter that counts output chunks and captures the last chunk.
    struct TestEmitter {
        count: AtomicUsize,
        last_id: Mutex<Option<String>>,
        last_data: Mutex<Vec<u8>>,
    }

    impl TestEmitter {
        fn new() -> Self {
            Self {
                count: AtomicUsize::new(0),
                last_id: Mutex::new(None),
                last_data: Mutex::new(Vec::new()),
            }
        }
    }

    impl TerminalEventEmitter for TestEmitter {
        fn emit_output(&self, terminal_id: &str, data: &[u8]) {
            self.count.fetch_add(1, Ordering::Relaxed);
            *self.last_id.lock().unwrap() = Some(terminal_id.to_string());
            *self.last_data.lock().unwrap() = data.to_vec();
        }
    }

    #[test]
    fn create_and_kill() {
        let emitter = Arc::new(TestEmitter::new());
        let mgr = TerminalManager::new(emitter.clone());
        let id = mgr.create(None).expect("spawn shell");
        assert!(!id.is_empty());
        // kill should succeed
        mgr.kill(&id).expect("kill");
        // second kill should fail (NotFound)
        assert!(mgr.kill(&id).is_err());
    }

    #[test]
    fn write_to_unknown_returns_not_found() {
        let emitter = Arc::new(TestEmitter::new());
        let mgr = TerminalManager::new(emitter);
        let err = mgr.write("nope", b"ls\n").unwrap_err();
        assert!(matches!(err, TerminalError::NotFound(_)));
    }

    #[test]
    fn resize_unknown_returns_not_found() {
        let emitter = Arc::new(TestEmitter::new());
        let mgr = TerminalManager::new(emitter);
        let err = mgr.resize("nope", 120, 40).unwrap_err();
        assert!(matches!(err, TerminalError::NotFound(_)));
    }

    #[test]
    fn kill_all_clears_everything() {
        let emitter = Arc::new(TestEmitter::new());
        let mgr = TerminalManager::new(emitter);
        let id1 = mgr.create(None).expect("spawn");
        let id2 = mgr.create(None).expect("spawn");
        mgr.kill_all();
        assert!(mgr.write(&id1, b"x").is_err());
        assert!(mgr.write(&id2, b"x").is_err());
    }

    #[test]
    fn default_shell_returns_something() {
        let (program, _) = default_shell();
        assert!(!program.is_empty());
    }
}
