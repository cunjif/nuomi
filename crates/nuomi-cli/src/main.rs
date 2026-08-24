//! nuomi — headless acceptance entrypoint for the harness kernel.
//!
//! Subcommands:
//!   nuomi run "<task>"      one-shot execution with streamed output
//!   nuomi resume <session>  resume an existing session
//!   nuomi repl              interactive session

fn main() -> std::process::ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match nuomi_cli::cli::parse_args(&args) {
        Ok(cmd) => cmd,
        Err(usage) => {
            eprintln!("{usage}");
            return std::process::ExitCode::from(2);
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("error: failed to start async runtime: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    match runtime.block_on(nuomi_cli::cli::execute(cmd)) {
        Ok(code) => std::process::ExitCode::from(code as u8),
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
