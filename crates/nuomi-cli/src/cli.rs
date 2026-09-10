//! CLI implementation: hand-rolled argument parsing (no clap), `run` /
//! `resume` / `repl` subcommands, streamed stdout echo and Ctrl+C handling
//! (SPEC T10, AC17–AC18).

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

use nuomi_core::domain::ProviderProtocol;
use nuomi_core::facade::{NuomiConfig, NuomiKernel, ProviderEndpoint};
use nuomi_core::plugins::DeltaCallback;

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
const DEFAULT_MODEL: &str = "gpt-4o-mini";
const DEFAULT_DB_FILE: &str = "nuomi.db";

pub const USAGE: &str = "\
nuomi — headless agent harness

USAGE:
  nuomi run \"<task>\" [--db <path>] [--base-url <url>] [--model <name>] [--protocol openai|anthropic]
  nuomi resume <session-id> \"<new input>\" [--db <path>] [--base-url <url>] [--model <name>] [--protocol openai|anthropic]
  nuomi repl   [--db <path>] [--base-url <url>] [--model <name>] [--protocol openai|anthropic]
  nuomi plugin list

REPL commands: /new  /sessions  /resume <session-id>  /exit
API key is read from the NUOMI_API_KEY environment variable.";

/// Shared connection options for all subcommands.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CliOptions {
    pub db: Option<PathBuf>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub protocol: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Run {
        task: String,
        opts: CliOptions,
    },
    Resume {
        session_id: String,
        input: String,
        opts: CliOptions,
    },
    Repl {
        opts: CliOptions,
    },
    /// `nuomi plugin list` — scan side-load dirs and print what would load
    /// (ADR 0009). Pure disk read: no database, no provider, no boot.
    PluginList,
}

/// Parses argv (without the program name). Returns `Err(usage)` on anything
/// malformed.
pub fn parse_args(args: &[String]) -> Result<Command, String> {
    let Some(sub) = args.first() else {
        return Err(USAGE.to_string());
    };
    let mut opts = CliOptions::default();
    let mut positionals: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let flag = args[i].as_str();
        let take_value = |i: &mut usize| -> Result<String, String> {
            *i += 1;
            args.get(*i)
                .cloned()
                .ok_or_else(|| format!("missing value for {flag}\n\n{USAGE}"))
        };
        match flag {
            "--db" => opts.db = Some(PathBuf::from(take_value(&mut i)?)),
            "--base-url" => opts.base_url = Some(take_value(&mut i)?),
            "--model" => opts.model = Some(take_value(&mut i)?),
            "--protocol" => opts.protocol = Some(take_value(&mut i)?),
            other => positionals.push(other.to_string()),
        }
        i += 1;
    }
    match sub.as_str() {
        "run" if !positionals.is_empty() => Ok(Command::Run {
            task: positionals.join(" "),
            opts,
        }),
        "resume" if positionals.len() >= 2 => Ok(Command::Resume {
            session_id: positionals.remove(0),
            input: positionals.join(" "),
            opts,
        }),
        "repl" if positionals.is_empty() => Ok(Command::Repl { opts }),
        "plugin" if positionals == vec!["list"] => Ok(Command::PluginList),
        _ => Err(USAGE.to_string()),
    }
}

fn parse_protocol(raw: Option<&str>) -> ProviderProtocol {
    match raw {
        Some("anthropic") => ProviderProtocol::AnthropicCompatible,
        _ => ProviderProtocol::OpenAiCompatible,
    }
}

/// Resolves the SQLite database path with priority: `--db` flag, then the
/// `NUOMI_DB_PATH` env value, then the default `nuomi.db` in the working
/// directory — so CLI and desktop shell share one session library when pointed
/// at the same file. The env value is passed in by the caller instead of read
/// here, keeping this pure and race-free under tests.
fn resolve_db_path(flag: Option<PathBuf>, env_value: Option<String>) -> PathBuf {
    match (flag, env_value) {
        (Some(db), _) => db,
        (None, Some(env)) if !env.trim().is_empty() => PathBuf::from(env),
        _ => PathBuf::from(DEFAULT_DB_FILE),
    }
}

async fn boot_kernel(opts: &CliOptions) -> anyhow::Result<NuomiKernel> {
    // CLI v1: key comes from the environment; never persisted or logged.
    let api_key = std::env::var("NUOMI_API_KEY").unwrap_or_default();
    let endpoint = ProviderEndpoint {
        protocol: parse_protocol(opts.protocol.as_deref()),
        base_url: opts
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
        api_key,
        model: opts
            .model
            .clone()
            .unwrap_or_else(|| DEFAULT_MODEL.to_string()),
    };
    let db_env = std::env::var("NUOMI_DB_PATH").ok();
    let db_path = resolve_db_path(opts.db.clone(), db_env);
    Ok(NuomiKernel::boot(NuomiConfig::new(db_path, endpoint)).await?)
}

/// Echoes streamed deltas to stdout with flush.
fn stdout_printer() -> DeltaCallback {
    Arc::new(|delta: String| {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(delta.as_bytes());
        let _ = out.flush();
    })
}

/// Executes a parsed command; returns the process exit code.
pub async fn execute(cmd: Command) -> anyhow::Result<i32> {
    match cmd {
        Command::Run { task, opts } => {
            let kernel = boot_kernel(&opts)
                .await?
                .with_delta_callback(Some(stdout_printer()));
            tokio::select! {
                res = kernel.run_task(&task) => match res {
                    Ok(result) => {
                        println!();
                        if result.truncated {
                            eprintln!("warning: max steps exhausted without a final answer");
                            Ok(1)
                        } else {
                            Ok(0)
                        }
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        Ok(1)
                    }
                },
                _ = tokio::signal::ctrl_c() => {
                    println!("\n[cancelled]");
                    Ok(130)
                }
            }
        }
        Command::Resume {
            session_id,
            input,
            opts,
        } => {
            let kernel = boot_kernel(&opts)
                .await?
                .with_delta_callback(Some(stdout_printer()));
            kernel.resume(&session_id).await?;
            tokio::select! {
                res = kernel.run_task(&input) => match res {
                    Ok(result) => {
                        println!();
                        if result.truncated { Ok(1) } else { Ok(0) }
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        Ok(1)
                    }
                },
                _ = tokio::signal::ctrl_c() => {
                    println!("\n[cancelled]");
                    Ok(130)
                }
            }
        }
        Command::Repl { opts } => repl_interactive(boot_kernel(&opts).await?).await,
        Command::PluginList => {
            print_plugin_list();
            Ok(0)
        }
    }
}

/// `nuomi plugin list` — scans the side-load directories (env / user config /
/// workspace) and prints what boot would load, skip or fail on, including the
/// declared permission surface (ADR 0009). Pure disk read, no boot.
fn print_plugin_list() {
    use nuomi_core::harness::sideload::{scan, LoadOutcome};
    let outcomes = scan(&[]);
    let mut loaded = 0usize;
    for outcome in &outcomes {
        match outcome {
            LoadOutcome::Loaded {
                source,
                dir,
                manifest,
            } => {
                loaded += 1;
                println!(
                    "loaded   {} {} (api v{}) [{}]",
                    manifest.id,
                    manifest.version,
                    manifest.api_version,
                    source.label()
                );
                println!("  dir: {}", dir.display());
                let p = &manifest.permissions;
                println!(
                    "  permissions: fs.read {:?} | fs.write {:?} | network {:?} | shell {}",
                    p.fs.read, p.fs.write, p.network, p.shell
                );
                if !manifest.tools.is_empty() {
                    let names: Vec<String> = manifest
                        .tools
                        .iter()
                        .map(|t| format!("{}.{}", manifest.id, t.name))
                        .collect();
                    println!("  tools: {}", names.join(", "));
                }
                if !manifest.hooks.is_empty() {
                    let points: Vec<&str> =
                        manifest.hooks.iter().map(|h| h.point.as_str()).collect();
                    println!("  hooks: {}", points.join(", "));
                }
                if !manifest.events.is_empty() {
                    let topics: Vec<&str> =
                        manifest.events.iter().map(|e| e.topic.as_str()).collect();
                    println!("  events: {}", topics.join(", "));
                }
                if let Some(editor) = &manifest.editor {
                    if !editor.has_no_contributions() {
                        let mut caps: Vec<String> = Vec::new();
                        if editor.hover {
                            caps.push("hover".into());
                        }
                        if editor.symbols {
                            caps.push("symbols".into());
                        }
                        caps.extend(
                            editor
                                .commands
                                .iter()
                                .map(|c| format!("/{}.{}", manifest.id, c.name)),
                        );
                        caps.extend(editor.overlays.iter().map(|o| format!("overlay:{}", o.id)));
                        if editor.languages.is_empty() {
                            println!("  editor: {}", caps.join(", "));
                        } else {
                            println!(
                                "  editor: {} | languages: {}",
                                caps.join(", "),
                                editor.languages.join(",")
                            );
                        }
                    }
                }
            }
            LoadOutcome::Skipped { dir, reason } => {
                println!("skipped  {} — {reason}", dir.display());
            }
            LoadOutcome::Failed { dir, reason } => {
                println!("failed   {} — {reason}", dir.display());
            }
        }
    }
    if outcomes.is_empty() {
        println!("no plugins found");
        println!("install into .nuomi/plugins/ (workspace), the user config dir,");
        println!("or list directories in NUOMI_PLUGIN_PATH — see docs/plugins/");
    } else {
        println!("{loaded} plugin(s) would load at boot");
    }
}

/// One REPL line's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplAction {
    Continue,
    Exit,
}

/// True when the line is a REPL meta-command or blank (not a task).
pub fn is_repl_command(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.is_empty() || trimmed.starts_with('/')
}

/// Handles one REPL line against the kernel, writing human-visible output to
/// `out`. Shared by the interactive loop and tests.
pub async fn handle_repl_line(
    kernel: &NuomiKernel,
    line: &str,
    out: &mut dyn Write,
) -> anyhow::Result<ReplAction> {
    let trimmed = line.trim();
    match trimmed {
        "" => Ok(ReplAction::Continue),
        "/exit" | "/quit" => {
            writeln!(out, "bye")?;
            Ok(ReplAction::Exit)
        }
        "/new" => {
            let id = kernel.new_session().await?;
            writeln!(out, "new session: {id}")?;
            Ok(ReplAction::Continue)
        }
        "/sessions" => {
            for s in kernel.list_sessions().await? {
                writeln!(out, "{}\t{}\tupdated {}", s.id, s.title, s.updated_at)?;
            }
            Ok(ReplAction::Continue)
        }
        other if other.starts_with("/resume") => {
            let id = other.split_whitespace().nth(1).unwrap_or_default();
            if id.is_empty() {
                writeln!(out, "usage: /resume <session-id>")?;
                return Ok(ReplAction::Continue);
            }
            match kernel.resume(id).await {
                Ok(()) => {
                    writeln!(out, "resumed {id}")?;
                    Ok(ReplAction::Continue)
                }
                Err(e) => {
                    writeln!(out, "error: {e}")?;
                    Ok(ReplAction::Continue)
                }
            }
        }
        other if other.starts_with('/') => {
            writeln!(
                out,
                "unknown command: {other} (try /new /sessions /resume <id> /exit)"
            )?;
            Ok(ReplAction::Continue)
        }
        task => match kernel.run_task(task).await {
            Ok(result) => {
                writeln!(out)?;
                writeln!(out, "{}", result.final_text)?;
                if result.truncated {
                    writeln!(out, "[truncated]")?;
                }
                Ok(ReplAction::Continue)
            }
            Err(e) => {
                writeln!(out, "error: {e}")?;
                Ok(ReplAction::Continue)
            }
        },
    }
}

/// Interactive REPL: stdin lines via a blocking reader thread, Ctrl+C at any
/// point prints `[cancelled]`; a Ctrl+C during a running task aborts it and
/// drops its resources (AC18).
async fn repl_interactive(kernel: NuomiKernel) -> anyhow::Result<i32> {
    println!("nuomi REPL — commands: /new /sessions /resume <id> /exit");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    tokio::task::spawn_blocking(move || {
        let stdin = std::io::stdin();
        let mut buf = String::new();
        loop {
            buf.clear();
            match stdin.read_line(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if tx.send(buf.trim_end().to_string()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let kernel = Arc::new(kernel);
    let mut stdout = std::io::stdout();
    loop {
        write!(stdout, "nuomi> ")?;
        stdout.flush()?;
        let line = tokio::select! {
            l = rx.recv() => match l {
                Some(l) => l,
                None => break,
            },
            _ = tokio::signal::ctrl_c() => {
                println!("\n[cancelled]");
                continue;
            }
        };

        if is_repl_command(&line) {
            match handle_repl_line(&kernel, &line, &mut stdout).await? {
                ReplAction::Continue => {}
                ReplAction::Exit => break,
            }
        } else {
            // Task execution: cancellable via Ctrl+C. Aborting drops the
            // in-flight future (HTTP streams close on drop — nothing to reap).
            let k = kernel.clone();
            let task = line.clone();
            print!("… ");
            stdout.flush()?;
            let handle = tokio::spawn(async move { k.run_task(&task).await });
            tokio::pin!(handle);
            tokio::select! {
                res = &mut handle => match res {
                    Ok(Ok(result)) => {
                        writeln!(stdout)?;
                        writeln!(stdout, "{}", result.final_text)?;
                    }
                    Ok(Err(e)) => writeln!(stdout, "error: {e}")?,
                    Err(_) => {}
                },
                _ = tokio::signal::ctrl_c() => {
                    handle.abort();
                    writeln!(stdout, "\n[cancelled]")?;
                }
            }
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_run_with_flags() {
        let cmd = parse_args(&args(&[
            "run",
            "fix the bug",
            "--db",
            "x.db",
            "--model",
            "deepseek-chat",
            "--protocol",
            "anthropic",
        ]))
        .unwrap();
        match cmd {
            Command::Run { task, opts } => {
                assert_eq!(task, "fix the bug");
                assert_eq!(opts.db, Some(PathBuf::from("x.db")));
                assert_eq!(opts.model.as_deref(), Some("deepseek-chat"));
                assert_eq!(opts.protocol.as_deref(), Some("anthropic"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn parses_resume_and_repl() {
        let cmd = parse_args(&args(&["resume", "sess-1", "continue please"])).unwrap();
        match cmd {
            Command::Resume {
                session_id, input, ..
            } => {
                assert_eq!(session_id, "sess-1");
                assert_eq!(input, "continue please");
            }
            other => panic!("unexpected command: {other:?}"),
        }
        assert_eq!(
            parse_args(&args(&["repl"])).unwrap(),
            Command::Repl {
                opts: CliOptions::default()
            }
        );
    }

    #[test]
    fn malformed_input_returns_usage() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&args(&["frobnicate"])).is_err());
        assert!(parse_args(&args(&["run"])).is_err()); // no task
        assert!(parse_args(&args(&["resume", "only-session"])).is_err());
        assert!(parse_args(&args(&["run", "task", "--db"])).is_err()); // dangling flag
    }

    #[test]
    fn protocol_defaults_to_openai() {
        assert_eq!(parse_protocol(None), ProviderProtocol::OpenAiCompatible);
        assert_eq!(
            parse_protocol(Some("anthropic")),
            ProviderProtocol::AnthropicCompatible
        );
    }

    /// Table-driven: `--db` > `NUOMI_DB_PATH` > `./nuomi.db`. The env value is
    /// an argument, so no process-global env mutation (no test races).
    #[test]
    fn resolve_db_path_priority_table() {
        let cases: &[(&str, Option<PathBuf>, Option<String>, PathBuf)] = &[
            (
                "flag wins over env",
                Some(PathBuf::from("flag.db")),
                Some("env.db".to_string()),
                PathBuf::from("flag.db"),
            ),
            (
                "env used when flag absent",
                None,
                Some("env.db".to_string()),
                PathBuf::from("env.db"),
            ),
            (
                "default when flag and env absent",
                None,
                None,
                PathBuf::from(DEFAULT_DB_FILE),
            ),
            (
                "empty env treated as unset",
                None,
                Some(String::new()),
                PathBuf::from(DEFAULT_DB_FILE),
            ),
        ];
        for (name, flag, env, expected) in cases {
            assert_eq!(
                resolve_db_path(flag.clone(), env.clone()),
                *expected,
                "{name}"
            );
        }
    }

    #[tokio::test]
    async fn repl_resume_switches_session_and_continues_transcript() {
        use nuomi_core::facade::NuomiConfig;
        use nuomi_core::providers::{ChatResponse, FakeLlm};

        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("r.db");
        let kernel = NuomiKernel::boot(NuomiConfig::with_fake_provider(
            db,
            vec![
                FakeLlm::response("first answer"),
                ChatResponse {
                    content: "continued answer".into(),
                    ..ChatResponse::default()
                },
            ],
        ))
        .await
        .unwrap();
        let original = kernel.session_id().await;
        kernel.run_task("hello").await.unwrap();

        // Start a fresh session, then resume the original via the REPL command.
        let mut out: Vec<u8> = Vec::new();
        handle_repl_line(&kernel, "/new", &mut out).await.unwrap();
        let action = handle_repl_line(&kernel, &format!("/resume {original}"), &mut out)
            .await
            .unwrap();
        assert_eq!(action, ReplAction::Continue);
        let echoed = String::from_utf8_lossy(&out).to_string();
        assert!(echoed.contains(&format!("resumed {original}")), "{echoed}");

        // The next task continues on top of the resumed history.
        let result = kernel.run_task("more").await.unwrap();
        assert_eq!(result.transcript.len(), 4); // u/a from turn 1 + new u/a
    }

    #[tokio::test]
    async fn repl_resume_without_id_prints_usage_and_unknown_commands_hint() {
        let dir = tempfile::tempdir().unwrap();
        let kernel = NuomiKernel::boot(nuomi_core::facade::NuomiConfig::with_fake_provider(
            dir.path().join("x.db"),
            vec![],
        ))
        .await
        .unwrap();
        let mut out: Vec<u8> = Vec::new();
        handle_repl_line(&kernel, "/resume", &mut out)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&out).contains("usage: /resume"));

        let mut out2: Vec<u8> = Vec::new();
        handle_repl_line(&kernel, "/frobnicate", &mut out2)
            .await
            .unwrap();
        assert!(
            String::from_utf8_lossy(&out2).contains("/resume <id>"),
            "{}",
            String::from_utf8_lossy(&out2)
        );
    }
}
