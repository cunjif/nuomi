//! PTY long-session adapter for interactive CLI agents
//! (harness-research-synthesis P1; see docs/research/parallel-code.md §2.5
//! and docs/research/agent-orchestrator.md §2.7).
//!
//! [`PtySession`] wraps a real pseudo-terminal (portable-pty: Windows ConPTY,
//! macOS/Linux unix PTYs) around a long-lived REPL agent (Claude Code, Codex,
//! a plain shell). It complements the one-shot [`super::cli`] adapter with:
//!
//! - an async output stream (`next_output`) backed by an mpsc channel;
//! - [`strip_ansi`] so downstream consumers and the detector see plain text;
//! - [`PromptDetector`]: per-agent prompt patterns + double confirmation
//!   (a pattern hit only counts as ready after a 50ms quiet period) +
//!   echo suppression (lines echoing what we wrote never count as prompt
//!   evidence — parallel-code `prompt-detect.ts` / `coordinator.ts:364`);
//! - [`DeliveryReadiness`]: the agent-orchestrator `WaitForMessageDeliveryReady`
//!   timing profile (150ms polling + 750ms stability window + 5s fallback).
//!   Delivery fails loudly rather than blindly writing into a busy session;
//! - `write_line` with automatic bracketed-paste wrapping when the agent
//!   enabled paste mode (`\x1b[?2004h` probed from the output stream).
//!
//! Resource semantics mirror the CLI adapter's `kill_on_drop`: dropping the
//! session kills the child and a detached reaper thread reaps it.
//!
//! Integration trade-off note: portable-pty's reader is a synchronous
//! `std::io::Read`. Blocking the tokio runtime on it is not acceptable, so a
//! dedicated OS thread pumps raw bytes into a tokio unbounded mpsc channel.
//! Backpressure is intentionally omitted: every byte must be consumed anyway
//! for prompt detection, so an unbounded channel cannot grow without bound
//! beyond what the agent actually emits.

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read as _, Write as _};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use super::AdapterError;
use thiserror::Error;

/// Errors from the PTY long-session adapter. Kept local to this module (not
/// merged into [`super::AdapterError`]) because that enum is exhaustively
/// matched by existing consumers (`cli.rs`); adding variants there would
/// break them. Errors convert into [`super::AdapterError`] via `From` for
/// future interop.
#[derive(Debug, Error)]
pub enum PtyError {
    /// Opening the pty pair failed.
    #[error("failed to open pty: {0}")]
    Open(String),

    /// Spawning the child inside the pty failed.
    #[error("failed to spawn pty child: {0}")]
    Spawn(String),

    /// Reading/writing the pty failed.
    #[error("pty io error: {0}")]
    Io(#[from] std::io::Error),

    /// Resizing the terminal failed.
    #[error("failed to resize pty: {0}")]
    Resize(String),

    /// Prompt readiness was not confirmed within the fallback timeout; the
    /// delivery was aborted rather than blindly writing into a busy session
    /// (agent-orchestrator WaitForMessageDeliveryReady semantics).
    #[error("prompt not ready within {timeout_ms}ms; delivery aborted")]
    PromptTimeout { timeout_ms: u64 },
}

impl From<PtyError> for AdapterError {
    fn from(e: PtyError) -> Self {
        match e {
            PtyError::Io(source) => AdapterError::Io(source),
            other => AdapterError::Protocol {
                agent: "pty".to_string(),
                message: other.to_string(),
            },
        }
    }
}

/// Sequence a terminal emits when the application enables bracketed-paste
/// mode (DECSET 2004). Its presence in the output stream means `write_line`
/// must wrap payloads in paste brackets.
const BRACKETED_PASTE_ENABLE: &str = "\x1b[?2004h";

/// Sequence a terminal emits when bracketed-paste mode is disabled.
const BRACKETED_PASTE_DISABLE: &str = "\x1b[?2004l";

/// Bracketed-paste start wrapper.
const PASTE_START: &str = "\x1b[200~";

/// Bracketed-paste end wrapper.
const PASTE_END: &str = "\x1b[201~";

/// Default terminal size for spawned sessions.
const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

/// Quiet period required after a pattern hit before the detector reports
/// ready (parallel-code `markAgentPromptReady` 50ms settling period).
const PROMPT_STABILITY_MS: u64 = 50;

/// Where a [`PromptPattern`]'s needle must appear for a line to match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptAnchor {
    /// The needle may appear anywhere in the line (e.g. `❯` mid-line after
    /// git-status segments in a starship prompt).
    Anywhere,
    /// The needle must end the line, allowing only trailing whitespace after
    /// it (covers the classic `$ ` / `> ` / `claude> ` prompt shapes and the
    /// "colon + whitespace" continuation form).
    LineEnd,
}

/// A single prompt-shape rule for [`PromptDetector`]. Deliberately substring
/// based (no regex dependency): prompt shapes are simple literals, and the
/// research notes patterns are inherently version-fragile and need to stay
/// trivially editable (parallel-code.md §2.5 "代价").
#[derive(Debug, Clone, Copy)]
pub struct PromptPattern {
    /// Stable identifier for diagnostics/config round-trips.
    pub name: &'static str,
    /// Literal substring to look for.
    pub needle: &'static str,
    /// Where the needle must appear.
    pub anchor: PromptAnchor,
}

impl PromptPattern {
    fn matches(&self, line: &str) -> bool {
        match self.anchor {
            PromptAnchor::Anywhere => line.contains(self.needle),
            PromptAnchor::LineEnd => line.trim_end().ends_with(self.needle),
        }
    }
}

/// Built-in prompt pattern table. Each entry names the CLI/shell family whose
/// prompt shape it matches; sources are the agents listed in
/// docs/specs/cli-agents-m1.md (claude_code / codex / plain dialects) plus
/// the generic shell forms observed by parallel-code's prompt-detect.
pub const PROMPT_PATTERNS: &[PromptPattern] = &[
    // starship / oh-my-zsh / powerlevel10k arrow — the dominant modern zsh
    // prompt and the primary shape parallel-code matches for agents running
    // inside an interactive shell (parallel-code prompt-detect.ts).
    PromptPattern {
        name: "zsh-arrow",
        needle: "❯",
        anchor: PromptAnchor::Anywhere,
    },
    // POSIX sh / bash default PS1: "user@host:~/repo$ ".
    PromptPattern {
        name: "sh-dollar",
        needle: "$",
        anchor: PromptAnchor::LineEnd,
    },
    // cmd.exe / PowerShell ("PS C:\repo>") and generic REPL `>` forms.
    PromptPattern {
        name: "shell-gt",
        needle: ">",
        anchor: PromptAnchor::LineEnd,
    },
    // Claude Code interactive REPL prompt (anthropics/claude-code).
    PromptPattern {
        name: "claude-code",
        needle: "claude>",
        anchor: PromptAnchor::LineEnd,
    },
    // OpenAI Codex CLI interactive prompt (openai/codex).
    PromptPattern {
        name: "codex",
        needle: "codex>",
        anchor: PromptAnchor::LineEnd,
    },
    // Generic REPL continuation: line ending in colon + whitespace, seen in
    // python REPL / input() style and interactive installers.
    PromptPattern {
        name: "colon-blank",
        needle: ":",
        anchor: PromptAnchor::LineEnd,
    },
];

/// Strips ANSI escape sequences from terminal output: CSI sequences
/// (`\x1b[...m`, `\x1b[?2004h`, ...), OSC sequences terminated by BEL or ST
/// (`\x1b]0;title\x07`, hyperlink form `\x1b]8;;url\x1b\\`), two-character
/// escapes (`\x1b=`, `\x1b7`) and charset designations (`\x1b(B`).
///
/// Implemented as a hand-rolled scanner (substring/byte logic, no regex
/// dependency). Unterminated sequences at end-of-input are dropped whole.
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // CSI: parameters (0x30..=0x3F) and intermediates (0x20..=0x2F),
            // terminated by a final byte in 0x40..=0x7E.
            Some('[') => {
                for next in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&next) {
                        break;
                    }
                }
            }
            // OSC: terminated by BEL or ST (ESC \).
            Some(']') => loop {
                match chars.next() {
                    None | Some('\x07') => break,
                    Some('\x1b') => {
                        let _ = chars.next();
                        break;
                    }
                    Some(_) => {}
                }
            },
            // Charset designation: one following byte selects the set.
            Some('(') | Some(')') => {
                let _ = chars.next();
            }
            // Any other two-character escape (ESC =, ESC >, ESC 7, ...).
            Some(_) => {}
            None => {}
        }
    }
    out
}

/// Pure probe used by [`PtySession`] to track bracketed-paste mode from raw
/// output chunks: `Some(true)` when paste mode was enabled, `Some(false)`
/// when disabled, `None` when the chunk says nothing about it.
fn probe_bracketed_paste(raw: &str) -> Option<bool> {
    if raw.contains(BRACKETED_PASTE_ENABLE) {
        Some(true)
    } else if raw.contains(BRACKETED_PASTE_DISABLE) {
        Some(false)
    } else {
        None
    }
}

/// Pure helper wrapping a payload in bracketed-paste sequences when the
/// terminal enabled paste mode; identity otherwise.
fn wrap_bracketed_paste(enabled: bool, line: &str) -> String {
    if enabled {
        format!("{PASTE_START}{line}{PASTE_END}")
    } else {
        line.to_string()
    }
}

/// Prompt-readiness state machine.
///
/// Feed it stripped output (`feed`) with a monotonic millisecond timestamp
/// and poll it (`poll`). A pattern hit arms the detector; readiness requires
/// a quiet period of [`PROMPT_STABILITY_MS`] after the *last* output
/// (double confirmation). Lines echoing text registered via [`Self::note_written`]
/// are skipped so our own prompt deliveries cannot be mistaken for an idle
/// prompt (parallel-code echo suppression).
#[derive(Debug, Clone)]
pub struct PromptDetector {
    patterns: Vec<PromptPattern>,
    /// Fragments of text we wrote whose echo must not count as evidence.
    /// Consumed one-shot on first matching line.
    echo_markers: Vec<String>,
    armed: bool,
    last_output_ms: Option<u64>,
    ready: bool,
}

impl PromptDetector {
    /// Builds a detector over `patterns` (see [`PROMPT_PATTERNS`] for the
    /// built-in table).
    pub fn new(patterns: &[PromptPattern]) -> Self {
        Self {
            patterns: patterns.to_vec(),
            echo_markers: Vec::new(),
            armed: false,
            last_output_ms: None,
            ready: false,
        }
    }

    /// Registers `text` as written by us: any output line containing it is
    /// treated as echo and skipped by detection. Also disarms the detector —
    /// a new prompt render is required before the next readiness.
    pub fn note_written(&mut self, text: &str) {
        let marker = text.trim();
        if !marker.is_empty() {
            self.echo_markers.push(marker.to_string());
        }
        self.armed = false;
        self.ready = false;
    }

    /// Feeds a chunk of ANSI-stripped output observed at `now_ms`.
    pub fn feed(&mut self, output: &str, now_ms: u64) {
        self.last_output_ms = Some(now_ms);
        for line in output.split('\n') {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            if self.consume_echo_marker(line) {
                continue;
            }
            if self.patterns.iter().any(|p| p.matches(line)) {
                self.armed = true;
            }
        }
    }

    /// Re-evaluates readiness at `now_ms`: armed (a pattern hit happened
    /// since the last write) AND no new output for [`PROMPT_STABILITY_MS`].
    pub fn poll(&mut self, now_ms: u64) -> bool {
        self.ready = self.armed
            && self
                .last_output_ms
                .is_some_and(|t| now_ms.saturating_sub(t) >= PROMPT_STABILITY_MS);
        self.ready
    }

    /// Last readiness verdict computed by [`Self::poll`].
    pub fn is_ready(&self) -> bool {
        self.ready
    }

    fn consume_echo_marker(&mut self, line: &str) -> bool {
        if let Some(idx) = self
            .echo_markers
            .iter()
            .position(|marker| line.contains(marker.as_str()))
        {
            self.echo_markers.swap_remove(idx);
            true
        } else {
            false
        }
    }
}

/// Polling source used by [`DeliveryReadiness::wait_ready`]. Implementors
/// drain newly available output into their detector and report the current
/// readiness verdict. Splitting this out keeps the timing profile testable
/// against a stub without a real PTY.
pub trait ReadinessPoller {
    /// Drains pending output into the detector, then reports whether the
    /// prompt is currently in the ready state.
    fn poll_ready(&mut self) -> bool;
}

/// Delivery-readiness window (agent-orchestrator §2.7
/// `WaitForMessageDeliveryReady`): poll every 150ms, require the prompt to
/// stay ready continuously for 750ms, and give up after 5s — a failed
/// delivery is recoverable, a prompt blindly typed into a busy agent is not.
#[derive(Debug, Clone)]
pub struct DeliveryReadiness {
    poll_interval: Duration,
    stability_window: Duration,
    fallback_timeout: Duration,
}

impl Default for DeliveryReadiness {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(150),
            stability_window: Duration::from_millis(750),
            fallback_timeout: Duration::from_millis(5_000),
        }
    }
}

impl DeliveryReadiness {
    /// Explicit timing configuration (tests and future tuning).
    pub fn new(
        poll_interval: Duration,
        stability_window: Duration,
        fallback_timeout: Duration,
    ) -> Self {
        Self {
            poll_interval,
            stability_window,
            fallback_timeout,
        }
    }

    /// Waits until `poller` has reported ready continuously for the
    /// stability window. Returns [`PtyError::PromptTimeout`] when the
    /// fallback timeout elapses first.
    ///
    /// Timing uses `tokio::time`, so tests can run this deterministically
    /// under `#[tokio::test(start_paused = true)]`.
    pub async fn wait_ready(&self, poller: &mut dyn ReadinessPoller) -> Result<(), PtyError> {
        let start = tokio::time::Instant::now();
        let mut ready_since: Option<tokio::time::Instant> = None;
        loop {
            if poller.poll_ready() {
                let since = ready_since.get_or_insert_with(tokio::time::Instant::now);
                if since.elapsed() >= self.stability_window {
                    return Ok(());
                }
            } else {
                ready_since = None;
            }
            if start.elapsed() >= self.fallback_timeout {
                return Err(PtyError::PromptTimeout {
                    timeout_ms: self.fallback_timeout.as_millis() as u64,
                });
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }
}

/// A long-lived PTY session around an interactive CLI agent process.
///
/// Output is pumped by a dedicated OS thread into an async channel (see the
/// module docs for the trade-off). `next_output` consumes chunks, strips
/// ANSI, tracks bracketed-paste mode and feeds the built-in
/// [`PromptDetector`]. Dropping the session kills and reaps the child.
pub struct PtySession {
    writer: Box<dyn std::io::Write + Send>,
    /// Kept alive for the lifetime of the session; dropping it would close
    /// the pty and silence the agent.
    _master: Box<dyn MasterPty + Send>,
    child: Option<Box<dyn portable_pty::Child + Send>>,
    output_rx: UnboundedReceiver<Vec<u8>>,
    detector: PromptDetector,
    bracketed_paste: Option<bool>,
    /// Clock base for detector timestamps (monotonic ms since spawn).
    clock_base: Instant,
}

impl PtySession {
    /// Opens a PTY (80x24) and spawns `command` with `args` inside it.
    ///
    /// `env` is overlaid onto the sanitized parent environment (same
    /// [`super::cli::ENV_BLOCK_LIST`] deny list as the CLI adapter — an
    /// interactive agent is as untrusted as a one-shot one). `cwd` is
    /// optional. Prompt detection uses the built-in [`PROMPT_PATTERNS`].
    pub fn spawn(
        command: &str,
        args: &[String],
        cwd: Option<&std::path::Path>,
        env: &[(String, String)],
    ) -> Result<Self, PtyError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: DEFAULT_ROWS,
                cols: DEFAULT_COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Open(e.to_string()))?;

        let mut cmd = CommandBuilder::new(command);
        cmd.args(args.iter().map(|a| a.as_str()));
        if let Some(dir) = cwd {
            cmd.cwd(dir);
        }
        cmd.env_clear();
        for (key, value) in build_child_env(env) {
            cmd.env(key, value);
        }

        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::Spawn(e.to_string()))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| PtyError::Open(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| PtyError::Open(e.to_string()))?;
        // Closing our slave handle lets the reader observe EOF once the child
        // exits (required on unix).
        drop(pair.slave);

        let (output_tx, output_rx) = unbounded_channel::<Vec<u8>>();
        // portable-pty's reader is synchronous; pump it from a dedicated OS
        // thread so the async runtime never blocks (module docs: trade-off).
        let _pump = std::thread::Builder::new()
            .name("nuomi-pty-pump".to_string())
            .spawn(move || {
                let mut reader = reader;
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if output_tx.send(buf[..n].to_vec()).is_err() {
                                break;
                            }
                        }
                    }
                }
            });

        Ok(Self {
            writer,
            _master: pair.master,
            child: Some(child),
            output_rx,
            detector: PromptDetector::new(PROMPT_PATTERNS),
            bracketed_paste: None,
            clock_base: Instant::now(),
        })
    }

    /// Receives the next chunk of output, ANSI-stripped. Returns `None` once
    /// the pty closed (child exited and all buffered chunks were consumed).
    pub async fn next_output(&mut self) -> Option<String> {
        let chunk = self.output_rx.recv().await?;
        Some(self.ingest(chunk))
    }

    /// Writes raw text to the pty (no newline, no wrapping).
    pub fn write_str(&mut self, text: &str) -> Result<(), PtyError> {
        self.writer
            .write_all(text.as_bytes())
            .and_then(|()| self.writer.flush())
            .map_err(PtyError::Io)
    }

    /// Writes `line` followed by Enter. When the agent enabled
    /// bracketed-paste mode (probed via `\x1b[?2004h`), the payload is
    /// wrapped in paste brackets so multi-stroke input arrives atomically.
    /// The line is registered for echo suppression.
    pub fn write_line(&mut self, line: &str) -> Result<(), PtyError> {
        let body = wrap_bracketed_paste(self.bracketed_paste == Some(true), line);
        self.write_str(&body)?;
        self.write_str("\r")?;
        self.detector.note_written(line);
        Ok(())
    }

    /// Resizes the terminal.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Result<(), PtyError> {
        self._master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Resize(e.to_string()))
    }

    /// Kills the child process. The session remains usable only for draining
    /// buffered output; the pty closes once the child dies.
    pub fn kill(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
        }
    }

    /// Waits for prompt readiness (150ms poll / 750ms stability / 5s
    /// fallback) and then delivers `text` as a prompt line.
    pub async fn send_prompt(&mut self, text: &str) -> Result<(), PtyError> {
        DeliveryReadiness::default().wait_ready(self).await?;
        self.write_line(text)
    }

    /// Prompt detector accessor (e.g. for diagnostics or custom flows).
    pub fn detector(&mut self) -> &mut PromptDetector {
        &mut self.detector
    }

    fn now_ms(&self) -> u64 {
        self.clock_base.elapsed().as_millis() as u64
    }

    fn ingest(&mut self, chunk: Vec<u8>) -> String {
        let raw = String::from_utf8_lossy(&chunk);
        if let Some(enabled) = probe_bracketed_paste(&raw) {
            self.bracketed_paste = Some(enabled);
        }
        let clean = strip_ansi(&raw);
        self.detector.feed(&clean, self.now_ms());
        clean
    }
}

impl ReadinessPoller for PtySession {
    fn poll_ready(&mut self) -> bool {
        let now = self.now_ms();
        // Drain everything that arrived since the last poll so the detector
        // sees the latest output before being polled.
        while let Ok(chunk) = self.output_rx.try_recv() {
            self.ingest(chunk);
        }
        self.detector.poll(now)
    }
}

impl Drop for PtySession {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            // Reap in the background so Drop never blocks on exit status.
            let _ = std::thread::Builder::new()
                .name("nuomi-pty-reap".to_string())
                .spawn(move || {
                    let _ = child.wait();
                });
        }
    }
}

/// Parent environment filtered through the shared deny list, overlaid with
/// `env` (which cannot re-introduce a blocked key — defense in depth).
fn build_child_env(env: &[(String, String)]) -> Vec<(String, String)> {
    let mut map: std::collections::HashMap<String, String> = std::env::vars()
        .filter(|(key, _)| !super::cli::is_env_blocked(key))
        .collect();
    for (key, value) in env {
        if super::cli::is_env_blocked(key) {
            continue;
        }
        map.insert(key.clone(), value.clone());
    }
    map.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    // ---- strip_ansi -------------------------------------------------------

    #[test]
    fn strip_ansi_passthrough_plain_text() {
        assert_eq!(strip_ansi("hello world\r\n"), "hello world\r\n");
    }

    #[test]
    fn strip_ansi_removes_sgr_color_sequences() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m plain"), "red plain");
        assert_eq!(strip_ansi("\x1b[1;32;40mbold\x1b[m"), "bold");
    }

    #[test]
    fn strip_ansi_removes_csi_private_modes() {
        // Bracketed-paste enable/disable and cursor visibility.
        assert_eq!(strip_ansi("\x1b[?2004h\x1b[?25ltext\x1b[?25h"), "text");
    }

    #[test]
    fn strip_ansi_removes_osc_with_bel_terminator() {
        assert_eq!(strip_ansi("\x1b]0;window title\x07after"), "after");
    }

    #[test]
    fn strip_ansi_removes_osc_with_st_terminator() {
        // OSC 8 hyperlink: ESC \ (ST) terminated.
        assert_eq!(
            strip_ansi("\x1b]8;;http://x\x1b\\link\x1b]8;;\x1b\\"),
            "link"
        );
    }

    #[test]
    fn strip_ansi_removes_charset_and_short_escapes() {
        assert_eq!(strip_ansi("\x1b(B\x1b=mid\x1b>end"), "midend");
    }

    #[test]
    fn strip_ansi_drops_unterminated_trailing_sequence() {
        assert_eq!(strip_ansi("text\x1b[31"), "text");
        assert_eq!(strip_ansi("text\x1b]0;abc"), "text");
    }

    // ---- PromptDetector ---------------------------------------------------

    fn detector() -> PromptDetector {
        PromptDetector::new(PROMPT_PATTERNS)
    }

    #[test]
    fn pattern_hit_requires_quiet_period() {
        let mut d = detector();
        d.feed("analyzing files\n$ ", 0);
        assert!(!d.poll(0));
        assert!(!d.poll(49));
        assert!(d.poll(50));
        assert!(d.is_ready());
    }

    #[test]
    fn new_output_resets_quiet_clock() {
        let mut d = detector();
        d.feed("$ ", 0);
        d.feed("working on it...", 30);
        assert!(!d.poll(79));
        assert!(d.poll(80));
    }

    #[test]
    fn no_pattern_hit_never_ready() {
        let mut d = detector();
        d.feed("plain output line\nmore text\n", 0);
        assert!(!d.poll(10_000));
    }

    #[test]
    fn line_end_anchor_requires_needle_at_line_end() {
        let mut d = PromptDetector::new(&[PromptPattern {
            name: "dollar",
            needle: "$",
            anchor: PromptAnchor::LineEnd,
        }]);
        d.feed("cost: 5 dollars spent\n", 0);
        assert!(!d.poll(1_000), "mid-line $ must not match LineEnd");
        d.feed("$ ", 0);
        assert!(d.poll(1_000));
    }

    #[test]
    fn echo_suppression_skips_written_lines() {
        let mut d = detector();
        d.note_written("list files >");
        // Our own echoed line ends with '>' — would be a false hit without
        // suppression.
        d.feed("list files >\n", 0);
        assert!(!d.poll(1_000));
        // A real prompt afterwards still arms the detector.
        d.feed("$ ", 2_000);
        assert!(d.poll(2_100));
    }

    #[test]
    fn note_written_disarms_previous_hit() {
        let mut d = detector();
        d.feed("$ ", 0);
        assert!(d.poll(1_000));
        d.note_written("next task");
        assert!(!d.poll(5_000), "write must invalidate prior readiness");
    }

    #[test]
    fn echo_marker_is_consumed_once() {
        let mut d = detector();
        d.note_written("TODO:");
        d.feed("TODO:\n", 0); // echo suppressed: trailing ':' must not arm
        assert!(!d.poll(1_000));
        // The marker is one-shot: identical later output is real output and
        // its trailing ':' counts as a (fragile but real) prompt hit.
        d.feed("TODO:\n", 2_000);
        assert!(d.poll(2_100));
    }

    // ---- bracketed paste helpers ------------------------------------------

    #[test]
    fn paste_wrap_only_when_enabled() {
        assert_eq!(wrap_bracketed_paste(true, "hi"), "\x1b[200~hi\x1b[201~");
        assert_eq!(wrap_bracketed_paste(false, "hi"), "hi");
    }

    #[test]
    fn paste_probe_tracks_enable_and_disable() {
        assert_eq!(probe_bracketed_paste(BRACKETED_PASTE_ENABLE), Some(true));
        assert_eq!(probe_bracketed_paste(BRACKETED_PASTE_DISABLE), Some(false));
        assert_eq!(probe_bracketed_paste("no sequences here"), None);
    }

    // ---- DeliveryReadiness -------------------------------------------------

    /// Deterministic stub: detector timestamps advance by the 150ms poll
    /// interval on every poll; tokio paused time drives the wall clock.
    struct StubPoller {
        detector: PromptDetector,
        now: u64,
    }

    impl ReadinessPoller for StubPoller {
        fn poll_ready(&mut self) -> bool {
            self.now += 150;
            self.detector.poll(self.now)
        }
    }

    fn readiness() -> DeliveryReadiness {
        DeliveryReadiness::new(
            Duration::from_millis(150),
            Duration::from_millis(750),
            Duration::from_millis(5_000),
        )
    }

    #[tokio::test(start_paused = true)]
    async fn wait_ready_requires_continuous_stability_window() {
        let mut poller = StubPoller {
            detector: detector(),
            now: 0,
        };
        poller.detector.feed("$ ", 0);
        // First polls below the 750ms window; overall well inside 5s fallback.
        readiness().wait_ready(&mut poller).await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn wait_ready_falls_back_to_error_on_silent_agent() {
        let mut poller = StubPoller {
            detector: detector(), // never fed: no pattern hit, never ready
            now: 0,
        };
        let err = readiness()
            .wait_ready(&mut poller)
            .await
            .expect_err("must time out");
        assert!(matches!(err, PtyError::PromptTimeout { timeout_ms: 5_000 }));
    }

    #[tokio::test(start_paused = true)]
    async fn wait_ready_resets_window_when_readiness_flaps() {
        struct FlappyPoller {
            calls: Cell<u32>,
        }
        impl ReadinessPoller for FlappyPoller {
            fn poll_ready(&mut self) -> bool {
                let n = self.calls.get();
                self.calls.set(n + 1);
                // Ready twice in a row, then not, alternating — the 750ms
                // continuous window must never be satisfied, forcing the
                // 5s fallback error.
                n % 3 != 2
            }
        }
        let mut poller = FlappyPoller {
            calls: Cell::new(0),
        };
        let err = readiness()
            .wait_ready(&mut poller)
            .await
            .expect_err("flapping readiness must never stabilize");
        assert!(matches!(err, PtyError::PromptTimeout { .. }));
    }

    // ---- real PTY (manual) --------------------------------------------------

    /// Manual smoke test against a real pty:
    /// `cargo test -p nuomi-core -- --ignored`
    #[test]
    #[ignore = "spawns a real PTY child; run manually with: cargo test -p nuomi-core -- --ignored"]
    fn real_pty_roundtrip() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            #[cfg(windows)]
            let mut session = PtySession::spawn(
                "cmd",
                &[
                    "/k".to_string(),
                    "echo".to_string(),
                    "PTY_ROUNDTRIP_OK".to_string(),
                ],
                None,
                &[],
            )
            .unwrap();
            #[cfg(not(windows))]
            let mut session = PtySession::spawn("sh", &["-i".to_string()], None, &[]).unwrap();

            let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
            let mut seen = String::new();
            loop {
                match tokio::time::timeout_at(deadline, session.next_output()).await {
                    Ok(Some(chunk)) => {
                        seen.push_str(&chunk);
                        #[cfg(windows)]
                        if seen.contains("PTY_ROUNDTRIP_OK") {
                            break;
                        }
                        #[cfg(not(windows))]
                        if !seen.trim().is_empty() {
                            break;
                        }
                    }
                    Ok(None) => panic!("pty closed before any output"),
                    Err(_) => {
                        // NOTE: in non-interactive contexts ConPTY children may
                        // die at DLL init (0xC0000142) before producing output;
                        // run this test from a real console.
                        #[cfg(windows)]
                        {
                            let status = session.child.take().map(|mut c| {
                                let _ = c.kill();
                                c.wait().ok()
                            });
                            panic!("timed out waiting for pty output; got: {seen:?}; child status: {status:?}");
                        }
                        #[cfg(not(windows))]
                        panic!("timed out waiting for pty output; got: {seen:?}");
                    }
                }
            }
            session.kill();
        });
    }
}
