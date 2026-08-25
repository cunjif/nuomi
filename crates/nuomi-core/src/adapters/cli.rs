//! External CLI agent adapter (SPEC: docs/specs/cli-agents-m1.md, task C3).
//!
//! Wraps an external CLI agent process ([`crate::domain::AgentProfile`],
//! adapter kind `cli`) behind the kernel-facing [`LlmProvider`] trait so it
//! can join Teams with zero orchestrator changes.
//!
//! Safety rules (SPEC D6):
//! - arguments are passed as an argv array, never through a shell;
//! - the executable must be on the caller-supplied allowlist (basename,
//!   case-insensitive comparison — Windows paths are case-insensitive);
//! - the child is spawned with `kill_on_drop(true)`, so cancelling or simply
//!   dropping the stream reaps the process;
//! - stderr only ever surfaces truncated inside error values — never logged.
//!
//! Prompt delivery (SPEC D5): each arg string containing `{prompt}` gets the
//! composed prompt substituted in place (still a single argv entry); when no
//! arg carries the placeholder, the prompt is written to stdin and the pipe
//! is closed.

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdout, Command};
use tokio::task::JoinHandle;

use crate::domain::{AgentProfile, CliFlavor};
use crate::providers::client::LlmProvider;
use crate::providers::types::{ChatRequest, ChatResponse, MessageRole, StreamEvent, Usage};
use crate::providers::ProviderError;

use super::AdapterError;

/// Upper bound for stderr excerpts embedded in error values.
const MAX_STDERR_CHARS: usize = 500;

/// An external CLI agent exposed as an [`LlmProvider`] team member.
#[derive(Debug)]
pub struct CliAgentClient {
    profile: AgentProfile,
    /// Lowercased executable basenames accepted at spawn time.
    allowlist: Vec<String>,
    default_cwd: Option<PathBuf>,
}

impl CliAgentClient {
    /// Validates the profile against the executable allowlist.
    ///
    /// The allowlist holds executable file names compared by basename,
    /// case-insensitively. An empty allowlist rejects every command.
    pub fn new(profile: AgentProfile, allowlist: Vec<String>) -> Result<Self, AdapterError> {
        if !is_allowlisted(&profile.command, &allowlist) {
            return Err(AdapterError::CommandNotAllowlisted {
                command: profile.command.clone(),
            });
        }
        Ok(Self {
            profile,
            allowlist: allowlist.iter().map(|a| a.trim().to_lowercase()).collect(),
            default_cwd: None,
        })
    }

    /// Builder-style default working directory used when the profile has no
    /// `working_dir`. `None` (the default) inherits the caller's cwd.
    pub fn with_default_cwd(mut self, cwd: PathBuf) -> Self {
        self.default_cwd = Some(cwd);
        self
    }

    pub fn profile(&self) -> &AgentProfile {
        &self.profile
    }

    /// Resolves everything needed to spawn synchronously so configuration
    /// errors surface deterministically before any stream polling.
    fn prepare(&self, prompt: &str) -> Result<Prepared, AdapterError> {
        let profile = &self.profile;
        // Defense in depth: re-check even though `new` already validated.
        if !is_allowlisted(&profile.command, &self.allowlist) {
            return Err(AdapterError::CommandNotAllowlisted {
                command: profile.command.clone(),
            });
        }
        let (args, has_placeholder) =
            resolve_args(&profile.args, prompt).map_err(|message| AdapterError::Protocol {
                agent: profile.name.clone(),
                message,
            })?;
        let envs = resolve_env(&profile.env).map_err(|message| AdapterError::Protocol {
            agent: profile.name.clone(),
            message,
        })?;
        Ok(Prepared {
            agent: profile.name.clone(),
            program: profile.command.clone(),
            args,
            envs,
            working_dir: profile
                .working_dir
                .as_ref()
                .map(PathBuf::from)
                .or_else(|| self.default_cwd.clone()),
            stdin_payload: (!has_placeholder).then(|| prompt.to_string()),
            flavor: profile.flavor,
        })
    }
}

/// Everything required to spawn one child invocation.
struct Prepared {
    agent: String,
    program: String,
    args: Vec<String>,
    envs: Vec<(String, String)>,
    working_dir: Option<PathBuf>,
    /// Set when no arg carried `{prompt}`: deliver the prompt via stdin.
    stdin_payload: Option<String>,
    flavor: CliFlavor,
}

/// Live child state owned by the stream; dropped ⇒ child killed.
struct ReadState {
    agent: String,
    child: Child,
    lines: tokio::io::Lines<BufReader<ChildStdout>>,
    stderr_tail: Option<JoinHandle<String>>,
    queue: VecDeque<StreamEvent>,
    acc: StreamAccumulator,
    flavor: CliFlavor,
    had_output: bool,
}

enum StepState {
    Spawn(Box<Prepared>),
    Read(Box<ReadState>),
    Done,
}

#[async_trait]
impl LlmProvider for CliAgentClient {
    fn id(&self) -> &str {
        &self.profile.id
    }

    async fn complete(&self, request: &ChatRequest) -> Result<ChatResponse, ProviderError> {
        let mut stream = self.stream(request);
        while let Some(event) = stream.next().await {
            match event? {
                StreamEvent::Completed(response) => return Ok(response),
                StreamEvent::TextDelta(_) => {}
            }
        }
        Err(ProviderError::Protocol {
            provider: "cli-agent",
            message: "stream ended without a Completed event".into(),
        })
    }

    fn stream(
        &self,
        request: &ChatRequest,
    ) -> BoxStream<'static, Result<StreamEvent, ProviderError>> {
        let prompt = compose_prompt(request);
        let agent = self.profile.name.clone();
        match self.prepare(&prompt) {
            Err(error) => {
                futures::stream::once(async move { Err(adapter_error(&agent, error)) }).boxed()
            }
            Ok(prepared) => futures::stream::try_unfold(
                StepState::Spawn(Box::new(prepared)),
                |state| async move { step(state).await },
            )
            .boxed(),
        }
    }
}

/// Drives the spawn/read/finalize state machine, yielding one event per call.
///
/// The loop advances through internal transitions (spawn, multi-delta lines,
/// EOF) until an event is ready to yield. Returning `Ok(None)` ends the
/// stream and drops the state, which kills the child (`kill_on_drop`) — the
/// stream is self-contained.
async fn step(state: StepState) -> Result<Option<(StreamEvent, StepState)>, ProviderError> {
    let mut pending = Some(state);
    loop {
        match pending.take() {
            None => return Ok(None),
            Some(StepState::Done) => return Ok(None),
            Some(StepState::Spawn(prepared)) => {
                let agent = prepared.agent.clone();
                match spawn_child(*prepared).await {
                    Ok(read_state) => pending = Some(StepState::Read(Box::new(read_state))),
                    Err(error) => return Err(adapter_error(&agent, error)),
                }
            }
            Some(StepState::Read(mut rs)) => {
                if let Some(event) = rs.queue.pop_front() {
                    return Ok(Some((event, StepState::Read(rs))));
                }
                match rs.lines.next_line().await {
                    Ok(Some(line)) => {
                        rs.had_output = true;
                        for delta in feed_line(rs.flavor, &line, &mut rs.acc) {
                            rs.queue.push_back(StreamEvent::TextDelta(delta));
                        }
                        pending = Some(StepState::Read(rs));
                    }
                    Ok(None) => {
                        return finish_read(*rs)
                            .await
                            .map(|event| Some((event, StepState::Done)))
                    }
                    Err(source) => {
                        return Err(ProviderError::CliAgent {
                            agent: rs.agent,
                            message: format!("reading cli agent output failed: {source}"),
                        })
                    }
                }
            }
        }
    }
}

async fn spawn_child(prepared: Prepared) -> Result<ReadState, AdapterError> {
    let mut command = Command::new(&prepared.program);
    command.args(&prepared.args);
    for (key, value) in &prepared.envs {
        command.env(key, value);
    }
    if let Some(dir) = &prepared.working_dir {
        command.current_dir(dir);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = command.spawn().map_err(|source| AdapterError::Spawn {
        command: prepared.program.clone(),
        source,
    })?;

    // Write the prompt to stdin (then close it) concurrently so a large
    // prompt cannot deadlock against an unread stdout pipe.
    if let Some(prompt) = prepared.stdin_payload {
        if let Some(mut stdin) = child.stdin.take() {
            tokio::spawn(async move {
                let _ = stdin.write_all(prompt.as_bytes()).await;
                let _ = stdin.flush().await;
                // Dropping the handle closes the pipe: agents relying on
                // stdin EOF see end-of-input.
            });
        }
    }

    let stdout = child.stdout.take().ok_or_else(|| AdapterError::Protocol {
        agent: prepared.agent.clone(),
        message: "child stdout pipe unavailable".into(),
    })?;
    // Drain stderr concurrently so the child never blocks on a full pipe.
    let stderr_tail = child.stderr.take().map(|stderr| {
        tokio::spawn(async move {
            let mut tail = String::new();
            let mut reader = BufReader::new(stderr);
            let _ = reader.read_to_string(&mut tail).await;
            tail
        })
    });

    Ok(ReadState {
        agent: prepared.agent,
        child,
        lines: BufReader::new(stdout).lines(),
        stderr_tail,
        queue: VecDeque::new(),
        acc: StreamAccumulator::default(),
        flavor: prepared.flavor,
        had_output: false,
    })
}

/// Waits for the child to exit and produces the terminal event.
///
/// A non-zero exit is tolerated when the agent produced output (CLI agents
/// sometimes exit with warning codes) but is an error when nothing was
/// written — in that case a truncated stderr excerpt is attached.
async fn finish_read(mut rs: ReadState) -> Result<StreamEvent, ProviderError> {
    let status = rs.child.wait().await;
    let stderr_tail = match rs.stderr_tail.take() {
        Some(handle) => handle.await.unwrap_or_default(),
        None => String::new(),
    };
    let agent = rs.agent.clone();
    let response = rs.acc.into_response();
    match status {
        Ok(status) if !status.success() && !rs.had_output => Err(ProviderError::CliAgent {
            agent,
            message: format!(
                "cli agent exited with {status} and produced no output; stderr: {}",
                truncate_chars(&stderr_tail, MAX_STDERR_CHARS)
            ),
        }),
        Ok(_) => Ok(StreamEvent::Completed(response)),
        Err(source) => Err(ProviderError::CliAgent {
            agent,
            message: format!("waiting for cli agent failed: {source}"),
        }),
    }
}

// ---------------------------------------------------------------------------
// Pure helpers (unit-tested below; no process, no I/O)
// ---------------------------------------------------------------------------

/// Flattens a chat request into the single prompt handed to the CLI agent:
/// optional system line first, then one `Role: content` section per message,
/// separated by blank lines.
fn compose_prompt(request: &ChatRequest) -> String {
    let mut sections: Vec<String> = Vec::new();
    if let Some(system) = &request.system_prompt {
        sections.push(format!("System: {system}"));
    }
    for message in &request.messages {
        let role = match message.role {
            MessageRole::System => "System",
            MessageRole::User => "User",
            MessageRole::Assistant => "Assistant",
            MessageRole::Tool => "Tool",
        };
        sections.push(format!("{role}: {}", message.content));
    }
    sections.join("\n\n")
}

/// Substitutes `{prompt}` inside each arg-template string.
///
/// Returns the resolved argv plus whether the placeholder appeared at all
/// (it decides stdin vs argv delivery). `request.tools` is intentionally
/// ignored throughout: CLI agents manage their own tools.
fn resolve_args(
    args_json: &serde_json::Value,
    prompt: &str,
) -> Result<(Vec<String>, bool), String> {
    let items = args_json
        .as_array()
        .ok_or_else(|| "args template must be a JSON array of strings".to_string())?;
    let mut resolved = Vec::with_capacity(items.len());
    let mut has_placeholder = false;
    for item in items {
        let raw = item
            .as_str()
            .ok_or_else(|| "args template entries must be strings".to_string())?;
        if raw.contains("{prompt}") {
            has_placeholder = true;
            resolved.push(raw.replace("{prompt}", prompt));
        } else {
            resolved.push(raw.to_string());
        }
    }
    Ok((resolved, has_placeholder))
}

/// Flattens the profile's JSON-object `env` into key/value pairs.
fn resolve_env(env_json: &serde_json::Value) -> Result<Vec<(String, String)>, String> {
    let map = env_json
        .as_object()
        .ok_or_else(|| "env must be a JSON object of strings".to_string())?;
    let mut pairs = Vec::with_capacity(map.len());
    for (key, value) in map {
        let text = value
            .as_str()
            .ok_or_else(|| format!("env value for '{key}' must be a string"))?;
        pairs.push((key.clone(), text.to_string()));
    }
    Ok(pairs)
}

fn is_allowlisted(command: &str, allowlist: &[String]) -> bool {
    if allowlist.is_empty() {
        return false;
    }
    let base = executable_base_name(command);
    allowlist
        .iter()
        .any(|entry| entry.trim().to_lowercase() == base)
}

/// Basename of an executable path, lowercased (Windows paths are
/// case-insensitive, so comparison must be too).
fn executable_base_name(command: &str) -> String {
    Path::new(command)
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| command.trim().to_lowercase())
}

/// Running parse state for one agent invocation.
#[derive(Debug, Default)]
struct StreamAccumulator {
    text: String,
    usage: Option<Usage>,
}

impl StreamAccumulator {
    fn into_response(self) -> ChatResponse {
        ChatResponse {
            content: self.text,
            tool_calls: Vec::new(),
            usage: self.usage,
            finish_reason: Some("stop".into()),
        }
    }
}

/// Feeds one stdout line into the accumulator, returning the text deltas the
/// line produced. Fault-tolerance iron rule: unknown event types and
/// malformed JSON lines are silently skipped, never surfaced as errors.
fn feed_line(flavor: CliFlavor, line: &str, acc: &mut StreamAccumulator) -> Vec<String> {
    match flavor {
        CliFlavor::ClaudeCode => feed_claude_code(line, acc),
        CliFlavor::Codex => feed_codex(line, acc),
        CliFlavor::Plain => feed_plain(line, acc),
    }
}

fn feed_plain(line: &str, acc: &mut StreamAccumulator) -> Vec<String> {
    if line.trim().is_empty() {
        return Vec::new();
    }
    let delta = format!("{line}\n");
    acc.text.push_str(&delta);
    vec![delta]
}

fn feed_claude_code(line: &str, acc: &mut StreamAccumulator) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return Vec::new();
    };
    match value.get("type").and_then(|t| t.as_str()) {
        Some("assistant") => {
            let mut deltas = Vec::new();
            if let Some(blocks) = value.pointer("/message/content").and_then(|c| c.as_array()) {
                for block in blocks {
                    if block.get("type").and_then(|t| t.as_str()) != Some("text") {
                        continue;
                    }
                    if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                        acc.text.push_str(text);
                        deltas.push(text.to_string());
                    }
                }
            }
            deltas
        }
        Some("result") => {
            // Terminal record: the final `result` replaces streamed deltas.
            if let Some(result) = value.get("result").and_then(|r| r.as_str()) {
                acc.text = result.to_string();
            }
            if let Some(usage) = parse_usage(value.get("usage")) {
                acc.usage = Some(usage);
            }
            Vec::new()
        }
        // system / stream_event / anything else: silently skipped.
        _ => Vec::new(),
    }
}

fn feed_codex(line: &str, acc: &mut StreamAccumulator) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return Vec::new();
    };
    match value.get("type").and_then(|t| t.as_str()) {
        Some("item.completed") => {
            let mut deltas = Vec::new();
            if let Some(item) = value.get("item") {
                if item.get("type").and_then(|t| t.as_str()) == Some("agent_message") {
                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                        acc.text.push_str(text);
                        deltas.push(text.to_string());
                    }
                }
            }
            deltas
        }
        Some("turn.completed") => {
            if let Some(usage) = parse_usage(value.get("usage")) {
                acc.usage = Some(usage);
            }
            Vec::new()
        }
        // thread.started and anything else: silently skipped.
        _ => Vec::new(),
    }
}

fn parse_usage(value: Option<&serde_json::Value>) -> Option<Usage> {
    let usage = value?;
    Some(Usage {
        prompt_tokens: usage.get("input_tokens")?.as_i64()?,
        completion_tokens: usage.get("output_tokens")?.as_i64()?,
    })
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        text.chars().take(max_chars).collect()
    }
}

fn adapter_error(agent: &str, error: AdapterError) -> ProviderError {
    match error {
        AdapterError::CommandNotAllowlisted { command } => ProviderError::CliAgent {
            agent: agent.to_string(),
            message: format!("command '{command}' is not in the executable allowlist"),
        },
        AdapterError::Spawn { command, source } => ProviderError::CliAgent {
            agent: agent.to_string(),
            message: format!("failed to spawn '{command}': {source}"),
        },
        AdapterError::Io(source) => ProviderError::CliAgent {
            agent: agent.to_string(),
            message: source.to_string(),
        },
        AdapterError::Protocol { agent, message } => ProviderError::CliAgent { agent, message },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::types::{ChatMessage, ToolDef};

    fn profile(command: &str, args: serde_json::Value) -> AgentProfile {
        AgentProfile {
            id: "cli-1".into(),
            name: "tester".into(),
            adapter: "cli".into(),
            flavor: CliFlavor::ClaudeCode,
            command: command.into(),
            args,
            env: serde_json::json!({}),
            working_dir: None,
            enabled: true,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn request(system: Option<&str>, messages: Vec<ChatMessage>) -> ChatRequest {
        ChatRequest {
            model: "m".into(),
            system_prompt: system.map(str::to_string),
            messages,
            tools: Vec::<ToolDef>::new(),
            temperature: None,
            max_tokens: None,
        }
    }

    #[test]
    fn compose_prompt_with_system_and_multiple_messages() {
        let req = request(
            Some("be terse"),
            vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
        );
        assert_eq!(
            compose_prompt(&req),
            "System: be terse\n\nUser: hi\n\nAssistant: hello"
        );
    }

    #[test]
    fn compose_prompt_without_system_covers_all_roles() {
        let req = request(
            None,
            vec![
                ChatMessage::user("q"),
                ChatMessage::tool_result("c1", "ans"),
            ],
        );
        assert_eq!(compose_prompt(&req), "User: q\n\nTool: ans");
    }

    #[test]
    fn compose_prompt_empty_request_is_empty_string() {
        assert_eq!(compose_prompt(&request(None, vec![])), "");
    }

    /// Runs a flavor parser over `lines`, collecting deltas + final state.
    fn run_parser(flavor: CliFlavor, lines: &[&str]) -> (Vec<String>, StreamAccumulator) {
        let mut acc = StreamAccumulator::default();
        let mut deltas = Vec::new();
        for line in lines {
            deltas.extend(feed_line(flavor, line, &mut acc));
        }
        (deltas, acc)
    }

    #[test]
    fn parsers_table_driven_valid_invalid_and_unknown_lines() {
        let claude_assistant_a = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hello "}]},"session_id":"s"}"#;
        let claude_assistant_b = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"world"},{"type":"tool_use","id":"t"}]}}"#;
        let claude_result = r#"{"type":"result","subtype":"success","result":"hello world","usage":{"input_tokens":3,"output_tokens":2}}"#;
        let codex_item = r#"{"type":"item.completed","item":{"type":"agent_message","text":"hi from codex"},"id":1}"#;
        let codex_turn =
            r#"{"type":"turn.completed","usage":{"input_tokens":5,"output_tokens":4}}"#;

        /// (lines to feed, expected deltas, expected accumulated text, expected usage)
        type ParserCase<'a> = (
            CliFlavor,
            Vec<&'a str>,
            Vec<&'a str>,
            &'a str,
            Option<(i64, i64)>,
        );

        let cases: Vec<ParserCase<'_>> = vec![
            (
                CliFlavor::ClaudeCode,
                vec![
                    r#"{"type":"system","subtype":"init"}"#,
                    "not json at all",
                    claude_assistant_a,
                    r#"{"type":"stream_event"}"#,
                    claude_assistant_b,
                    "",
                    claude_result,
                ],
                vec!["hello ", "world"],
                "hello world",
                Some((3, 2)),
            ),
            (
                CliFlavor::Codex,
                vec![
                    r#"{"type":"thread.started","thread_id":"t"}"#,
                    "{broken json",
                    codex_item,
                    r#"{"type":"item.completed","item":{"type":"other"}}"#,
                    codex_turn,
                ],
                vec!["hi from codex"],
                "hi from codex",
                Some((5, 4)),
            ),
            (
                CliFlavor::Plain,
                vec!["alpha", "", "  ", "beta"],
                vec!["alpha\n", "beta\n"],
                "alpha\nbeta\n",
                None,
            ),
        ];

        for (flavor, lines, want_deltas, want_text, want_usage) in &cases {
            let (deltas, acc) = run_parser(*flavor, lines);
            assert_eq!(deltas, want_deltas.to_vec(), "{flavor:?} deltas");
            assert_eq!(acc.text, *want_text, "{flavor:?} accumulated text");
            match (&acc.usage, want_usage) {
                (None, None) => {}
                (Some(u), Some((inp, out))) => {
                    assert_eq!((u.prompt_tokens, u.completion_tokens), (*inp, *out));
                }
                _ => panic!("{flavor:?}: unexpected usage {:?}", acc.usage),
            }
        }
    }

    #[test]
    fn claude_code_result_terminal_value_overrides_accumulated_deltas() {
        let mut acc = StreamAccumulator::default();
        feed_line(
            CliFlavor::ClaudeCode,
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"partial"}]}}"#,
            &mut acc,
        );
        feed_line(
            CliFlavor::ClaudeCode,
            r#"{"type":"result","subtype":"success","result":"final answer","usage":{"input_tokens":9,"output_tokens":1}}"#,
            &mut acc,
        );
        assert_eq!(acc.text, "final answer");
        let response = acc.into_response();
        assert_eq!(response.content, "final answer");
        assert_eq!(response.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn empty_input_completes_with_empty_content() {
        for flavor in [CliFlavor::ClaudeCode, CliFlavor::Codex, CliFlavor::Plain] {
            let (_, acc) = run_parser(flavor, &[]);
            let response = acc.into_response();
            assert_eq!(response.content, "", "{flavor:?}");
            assert!(response.usage.is_none(), "{flavor:?}");
            assert_eq!(
                response.finish_reason.as_deref(),
                Some("stop"),
                "{flavor:?}"
            );
        }
    }

    #[test]
    fn allowlist_accepts_listed_basename() {
        assert!(
            CliAgentClient::new(profile("node", serde_json::json!([])), vec!["node".into()])
                .is_ok()
        );
    }

    #[test]
    fn allowlist_rejects_unlisted_command_with_command_in_message() {
        let err = CliAgentClient::new(
            profile("node.exe", serde_json::json!([])),
            vec!["python".into()],
        )
        .expect_err("must reject");
        assert!(
            matches!(err, AdapterError::CommandNotAllowlisted { .. }),
            "{err}"
        );
        assert!(err.to_string().contains("node.exe"), "{err}");
    }

    #[test]
    fn empty_allowlist_rejects_every_command() {
        let err = CliAgentClient::new(profile("node", serde_json::json!([])), vec![])
            .expect_err("must reject");
        assert!(matches!(err, AdapterError::CommandNotAllowlisted { .. }));
    }

    #[test]
    fn allowlist_comparison_is_case_insensitive_for_windows_paths() {
        let mixed_case = profile(r"C:\Tools\Node.EXE", serde_json::json!([]));
        assert!(
            CliAgentClient::new(mixed_case, vec!["NODE.exe".into()]).is_ok(),
            "basename compare must ignore case"
        );
    }

    #[test]
    fn resolve_args_substitutes_placeholder_and_reports_presence() {
        let (args, has) =
            resolve_args(&serde_json::json!(["-p", "{prompt}", "--verbose"]), "do it").unwrap();
        assert_eq!(
            args,
            vec![
                "-p".to_string(),
                "do it".to_string(),
                "--verbose".to_string()
            ]
        );
        assert!(has);

        let (args, has) = resolve_args(&serde_json::json!(["--json"]), "do it").unwrap();
        assert_eq!(args, vec!["--json".to_string()]);
        assert!(!has);
    }

    #[test]
    fn resolve_args_rejects_non_array_templates() {
        assert!(resolve_args(&serde_json::json!("x"), "p").is_err());
        assert!(resolve_args(&serde_json::json!([1]), "p").is_err());
    }

    #[test]
    fn resolve_env_requires_object_of_strings() {
        assert!(resolve_env(&serde_json::json!({"A": "b"})).is_ok());
        assert!(resolve_env(&serde_json::json!([])).is_err());
        assert!(resolve_env(&serde_json::json!({"A": 3})).is_err());
    }
}
