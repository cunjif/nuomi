//! nuomi-trampoline — Relay launcher (ADR-0025 §6.1, permanently frozen M7).
//!
//! Workflow: wait for host PID → verify SHA-256 → atomically replace →
//! relaunch. Responsibilities are permanently frozen; this binary never
//! self-updates and never assumes version negotiation duties.
//!
//! Usage:
//!   nuomi-trampoline \
//!     --pid <PID> \
//!     --new-binary <path> \
//!     --target <path> \
//!     --hash <sha256-hex> \
//!     [--archive-dir <path>] \
//!     [--timeout <secs>] \
//!     [-- <launch-args...>]

mod replace;

use std::path::PathBuf;
use std::time::Duration;

use replace::{atomic_replace, launch, verify_hash, wait_for_pid};

const USAGE: &str = "\
nuomi-trampoline — relay launcher (ADR-0025 §6.1)

USAGE:
  nuomi-trampoline --pid <PID> --new-binary <path> --target <path> --hash <sha256> [--archive-dir <path>] [--timeout <secs>] [-- <args...>]

OPTIONS:
  --pid <PID>          Process ID to wait for (required).
  --new-binary <path>  Path to the new binary to install (required).
  --target <path>      Path of the binary to replace (required).
  --hash <sha256>      Expected SHA-256 hex digest of the new binary (required).
  --archive-dir <path> Directory to archive the old binary (default: <target_dir>/archive).
  --timeout <secs>     Max seconds to wait for PID (default: 30).
  -- <args...>         Extra arguments to pass to the launched binary.
";

struct TrampolineArgs {
    pid: u32,
    new_binary: PathBuf,
    target: PathBuf,
    hash: String,
    archive_dir: PathBuf,
    timeout: Duration,
    launch_args: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<TrampolineArgs, String> {
    let mut pid: Option<u32> = None;
    let mut new_binary: Option<PathBuf> = None;
    let mut target: Option<PathBuf> = None;
    let mut hash: Option<String> = None;
    let mut archive_dir: Option<PathBuf> = None;
    let mut timeout_secs: u64 = 30;
    let mut launch_args: Vec<String> = Vec::new();

    let mut i = 0;
    let mut after_double_dash = false;
    while i < args.len() {
        let arg = &args[i];
        if after_double_dash {
            launch_args.push(arg.clone());
            i += 1;
            continue;
        }
        match arg.as_str() {
            "--" => {
                after_double_dash = true;
                i += 1;
            }
            "--pid" => {
                i += 1;
                let v = args.get(i).ok_or("--pid requires a value")?;
                pid = Some(v.parse().map_err(|_| "invalid PID")?);
                i += 1;
            }
            "--new-binary" => {
                i += 1;
                let v = args.get(i).ok_or("--new-binary requires a path")?;
                new_binary = Some(PathBuf::from(v));
                i += 1;
            }
            "--target" => {
                i += 1;
                let v = args.get(i).ok_or("--target requires a path")?;
                target = Some(PathBuf::from(v));
                i += 1;
            }
            "--hash" => {
                i += 1;
                let v = args.get(i).ok_or("--hash requires a value")?;
                hash = Some(v.clone());
                i += 1;
            }
            "--archive-dir" => {
                i += 1;
                let v = args.get(i).ok_or("--archive-dir requires a path")?;
                archive_dir = Some(PathBuf::from(v));
                i += 1;
            }
            "--timeout" => {
                i += 1;
                let v = args.get(i).ok_or("--timeout requires a value")?;
                timeout_secs = v.parse().map_err(|_| "invalid timeout")?;
                i += 1;
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            _ => return Err(format!("unknown argument: {arg}\n\n{USAGE}")),
        }
    }

    let pid = pid.ok_or("--pid is required")?;
    let new_binary = new_binary.ok_or("--new-binary is required")?;
    let target = target.ok_or("--target is required")?;
    let hash = hash.ok_or("--hash is required")?;
    let archive_dir = archive_dir.unwrap_or_else(|| {
        let dir = target.parent().unwrap_or(std::path::Path::new("."));
        dir.join("archive")
    });

    Ok(TrampolineArgs {
        pid,
        new_binary,
        target,
        hash,
        archive_dir,
        timeout: Duration::from_secs(timeout_secs),
        launch_args,
    })
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    eprintln!(
        "[trampoline] waiting for PID {} (timeout {}s)",
        parsed.pid,
        parsed.timeout.as_secs()
    );
    if let Err(e) = wait_for_pid(parsed.pid, parsed.timeout) {
        eprintln!("[trampoline] error: {e}");
        anyhow::bail!(e);
    }

    eprintln!(
        "[trampoline] verifying SHA-256 of {}",
        parsed.new_binary.display()
    );
    if let Err(e) = verify_hash(&parsed.new_binary, &parsed.hash) {
        eprintln!("[trampoline] error: {e}");
        anyhow::bail!(e);
    }

    eprintln!(
        "[trampoline] replacing {} with {} (archive: {})",
        parsed.target.display(),
        parsed.new_binary.display(),
        parsed.archive_dir.display()
    );
    let archive_path = match atomic_replace(&parsed.new_binary, &parsed.target, &parsed.archive_dir)
    {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[trampoline] error: {e}");
            anyhow::bail!(e);
        }
    };
    if !archive_path.as_os_str().is_empty() {
        eprintln!(
            "[trampoline] old binary archived to {}",
            archive_path.display()
        );
    }

    eprintln!("[trampoline] launching {}", parsed.target.display());
    if let Err(e) = launch(&parsed.target, &parsed.launch_args) {
        eprintln!("[trampoline] launch error: {e}");
        anyhow::bail!(e);
    }

    eprintln!("[trampoline] done");
    Ok(())
}
