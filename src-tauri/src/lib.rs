//! Nuomi desktop shell: thin Tauri layer over nuomi-core.
//!
//! - Commands: `#[tauri::command]` wrappers that delegate to `commands::*`
//!   free functions (testable without a Tauri runtime).
//! - Contracts: single tauri-specta builder; TS bindings exported to
//!   `../src/lib/ipc/bindings.gen.ts`.
//! - Events: kernel bus → ADR-0002 multi-channel emit (see `events.rs`).

pub mod commands;
mod events;
mod ipc_error;
pub mod notifier;
pub mod schedule_dispatcher;
pub mod state;
mod tauri_cmds;

pub use ipc_error::{codes, IpcError};

use tauri::{Emitter, Manager};
use tauri_specta::{collect_commands, Builder};

use nuomi_core::facade::ProviderSource;
use state::AppState;

/// Emitted once the kernel is booted, managed into Tauri and all background
/// services (event bridge / scheduler / notifier) are running. The frontend
/// gates IPC-dependent views on this event (with a query-poll fallback in
/// case the webview loads after the event fired).
pub const KERNEL_READY_EVENT: &str = "kernel-ready";
/// Emitted when the async kernel boot failed; payload is the error string.
pub const KERNEL_FAILED_EVENT: &str = "kernel-failed";

// ---------- contracts ----------

// The single specta collection point (ipc-contract rule).
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        tauri_cmds::create_session,
        tauri_cmds::list_sessions,
        tauri_cmds::resume_session,
        tauri_cmds::list_events,
        tauri_cmds::submit_task,
        tauri_cmds::create_task,
        tauri_cmds::list_tasks,
        tauri_cmds::update_task_status,
        tauri_cmds::delete_task,
        tauri_cmds::get_run,
        tauri_cmds::list_runs_by_task,
        tauri_cmds::list_pending_approvals,
        tauri_cmds::resolve_approval,
        tauri_cmds::list_dir,
        tauri_cmds::read_file,
        tauri_cmds::write_file,
        tauri_cmds::git_status,
        tauri_cmds::git_log,
        tauri_cmds::git_stage,
        tauri_cmds::git_commit,
        tauri_cmds::git_push,
        tauri_cmds::git_worktrees,
        tauri_cmds::git_diff,
        tauri_cmds::create_schedule,
        tauri_cmds::list_schedules,
        tauri_cmds::toggle_schedule,
        tauri_cmds::delete_schedule,
        tauri_cmds::upsert_provider,
        tauri_cmds::list_providers,
        tauri_cmds::delete_provider,
        tauri_cmds::test_provider_connection,
        tauri_cmds::list_provider_models,
        tauri_cmds::set_sensitive_tools,
        tauri_cmds::get_sensitive_tools,
        tauri_cmds::set_online_authorized,
        tauri_cmds::get_online_authorized,
        tauri_cmds::get_workspace,
        tauri_cmds::set_workspace,
        tauri_cmds::list_workspaces,
        tauri_cmds::add_workspace,
        tauri_cmds::remove_workspace,
        tauri_cmds::activate_workspace,
        tauri_cmds::get_active_workspace,
        tauri_cmds::list_orphan_sessions,
        tauri_cmds::reclaim_orphan_sessions,
        tauri_cmds::list_agent_profiles,
        tauri_cmds::upsert_agent_profile,
        tauri_cmds::delete_agent_profile,
        tauri_cmds::check_cli_agent,
        tauri_cmds::list_roles,
        tauri_cmds::upsert_role,
        tauri_cmds::delete_role,
        tauri_cmds::seed_builtin_roles,
        tauri_cmds::generate_role,
        tauri_cmds::get_routing_rules,
        tauri_cmds::set_routing_rules,
        tauri_cmds::route_capability,
        tauri_cmds::journal_rollback,
        tauri_cmds::list_teams,
        tauri_cmds::upsert_team,
        tauri_cmds::delete_team,
        tauri_cmds::list_whiteboard_notes,
        tauri_cmds::form_team,
        tauri_cmds::preview_team,
        tauri_cmds::run_team_on_task,
        tauri_cmds::run_team_session,
        tauri_cmds::list_integrations,
        tauri_cmds::upsert_integration,
        tauri_cmds::delete_integration,
        tauri_cmds::test_integration,
        tauri_cmds::plugin_list,
        tauri_cmds::plugin_install_from_path,
        tauri_cmds::plugin_uninstall,
        tauri_cmds::plugin_open_dir,
        tauri_cmds::plugin_editor_call,
        tauri_cmds::app_setting_get,
        tauri_cmds::app_setting_set,
        tauri_cmds::create_conversation,
        tauri_cmds::list_conversations,
        tauri_cmds::get_conversation,
        tauri_cmds::set_conversation_agent,
        tauri_cmds::list_agent_options,
        tauri_cmds::update_conversation,
        tauri_cmds::add_conversation_agent,
        tauri_cmds::get_agent_detail,
        tauri_cmds::submit_message,
        tauri_cmds::stop_conversation,
        tauri_cmds::list_active_runs,
        tauri_cmds::cancel_run,
        tauri_cmds::save_attachment,
        tauri_cmds::list_attachments,
        tauri_cmds::delete_attachment,
        tauri_cmds::upsert_schedule,
        tauri_cmds::update_schedule,
        tauri_cmds::inject_context,
        tauri_cmds::list_injectable_sessions,
        tauri_cmds::list_injectable_rules,
        tauri_cmds::transcribe_audio,
        tauri_cmds::list_asr_models,
    ])
}

/// Standalone bindings export (`pnpm contracts:gen`).
pub fn export_bindings() -> Result<(), Box<dyn std::error::Error>> {
    let ts = specta_typescript::Typescript::default()
        .bigint(specta_typescript::BigIntExportBehavior::Number)
        .header("// @ts-nocheck\n// GENERATED BY `pnpm contracts:gen` — DO NOT EDIT.");
    // Render to a string first and skip the file write when the content is
    // unchanged: rewriting on every debug launch would bump the mtime and
    // make the `tauri dev` file watcher restart the app in an endless loop.
    let rendered = specta_builder().export_str(&ts)?;
    // Anchor at the repo root via CARGO_MANIFEST_DIR (`src-tauri/`): a plain
    // relative path resolves against the process cwd, which is `src-tauri/`
    // under `tauri dev` but the repo root under `pnpm contracts:gen` — that
    // mismatch once wrote the bindings into the wrong tree on every launch
    // and made the dev watcher restart the app in an endless loop.
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lib/ipc/bindings.gen.ts");
    if std::fs::read_to_string(&path)
        .map(|current| current == rendered)
        .unwrap_or(true)
    {
        return Ok(());
    }
    std::fs::write(&path, rendered)?;
    ts.format(&path).ok();
    Ok(())
}

/// Forwards one kernel event onto the right Tauri channel(s).
async fn forward_event(
    app: tauri::AppHandle,
    mut rx: tokio::sync::broadcast::Receiver<nuomi_core::harness::Event>,
) {
    loop {
        match rx.recv().await {
            Ok(ev) => {
                for (channel, wire) in events::partition_event(&ev) {
                    if let Err(e) = app.emit(&channel, wire) {
                        tracing::warn!(error = %e, channel = %channel, "emit failed");
                    }
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!(skipped = n, "event bridge lagged");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Spawns a background loop that tails `change_log` and publishes
/// `change.<table>` domain events so the frontend can invalidate queries
/// in real time (plan §5.2 — CDC tail bridge).
fn spawn_change_stream(bus: nuomi_core::harness::EventBus, db_path: std::sync::Arc<str>) {
    tauri::async_runtime::spawn(async move {
        // One connection for the loop's whole life — it polls twice a second
        // for as long as the app runs.
        let mut conn = match open_tail_conn(&db_path).await {
            Ok(db) => Some(db),
            Err(e) => {
                tracing::warn!(error = %e, "change_stream failed to open the database");
                None
            }
        };
        // Start at the current high-water mark: replaying the whole history
        // on every boot would flood the frontend with stale `change.*` events
        // (the table is never pruned).
        let mut cursor = match conn.as_ref() {
            Some(db) => latest_change_seq(db),
            None => 0,
        };
        loop {
            let path = db_path.clone();
            let taken = conn.take();
            let polled = tokio::task::spawn_blocking(move || {
                let db = match taken {
                    Some(db) => db,
                    None => match nuomi_core::store::Db::open(&path) {
                        Ok(db) => db,
                        Err(e) => return (None, Err(e)),
                    },
                };
                let entries = nuomi_core::store::repos::change_log::tail_after(&db.0, cursor, 200);
                (Some(db), entries)
            })
            .await;

            match polled {
                Ok((db, Ok(entries))) => {
                    conn = db;
                    if entries.is_empty() {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        continue;
                    }
                    for entry in &entries {
                        let topic = format!("change.{}", entry.table_name);
                        let payload = serde_json::json!({
                            "rowId": entry.row_id,
                            "op": entry.op.as_str(),
                        });
                        bus.publish(nuomi_core::harness::Event::new(topic, payload));
                    }
                    cursor = entries.last().map(|e| e.seq).unwrap_or(cursor);
                    // Yield between batches: a large backlog must not starve
                    // the async runtime (or the webview) while it drains.
                    tokio::task::yield_now().await;
                }
                Ok((db, Err(e))) => {
                    conn = db;
                    tracing::warn!(error = %e, "change_stream tail failed");
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "change_stream join failed");
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }
    });
}

async fn open_tail_conn(
    db_path: &std::sync::Arc<str>,
) -> Result<nuomi_core::store::Db, nuomi_core::store::StoreError> {
    let path = db_path.clone();
    tokio::task::spawn_blocking(move || {
        let db = nuomi_core::store::Db::open(&path)?;
        nuomi_core::store::migrations::run(&db.0)?;
        Ok::<_, nuomi_core::store::StoreError>(db)
    })
    .await
    .unwrap_or_else(|e| {
        Err(nuomi_core::store::StoreError::Sqlite(
            rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
        ))
    })
}

/// High-water mark of `change_log`: the stream only publishes changes made
/// after boot.
fn latest_change_seq(db: &nuomi_core::store::Db) -> i64 {
    db.0.query_row("SELECT COALESCE(MAX(seq), 0) FROM change_log", [], |row| {
        row.get(0)
    })
    .unwrap_or(0)
}

/// Installs the fmt tracing subscriber honoring `RUST_LOG` (default `info`).
/// Safe to call multiple times; a second install is a no-op.
fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).try_init();
}

/// Bootstraps the kernel off the window-critical path: runs inside a task on
/// the Tauri async runtime while the main thread returns to the event loop
/// and the (config-defined) window paints immediately. Once the state is
/// ready it is managed into Tauri, background services start, `kernel-ready`
/// is emitted and provider warming is kicked off as a detached task.
async fn boot_and_wire(
    app: tauri::AppHandle,
    db_path: std::path::PathBuf,
    provider: ProviderSource,
    default_workspace: std::path::PathBuf,
) {
    let started = std::time::Instant::now();
    match AppState::boot(db_path, provider, default_workspace).await {
        Ok(state) => {
            let boot_ms = started.elapsed().as_millis() as u64;

            // Event bridge: kernel bus → ADR-0002 channels.
            let rx = state.kernel.context().subscribe();
            tauri::async_runtime::spawn(forward_event(app.clone(), rx));

            // CDC change stream: tail change_log → domain events (plan §5.2).
            spawn_change_stream(state.kernel.context().bus(), state.db_path.clone());

            // Scheduler background runner + notification dispatcher. Both
            // spawn tokio tasks internally — we are already inside the
            // async runtime context here (no block_on needed).
            let runner = nuomi_core::services::SchedulerRunner::new(
                state.db_path.clone(),
                std::time::Duration::from_secs(5),
            )
            .with_bus(state.kernel.context().bus())
            .spawn();
            app.manage(runner);
            notifier::spawn(&state);
            schedule_dispatcher::spawn(&state);

            let seed_db = state.db_path.clone();
            let warm_db = state.db_path.clone();
            app.manage(state);

            // First-launch preset seeding: idempotent upsert of the built-in
            // role catalog. Off the critical path; failures are logged only.
            tauri::async_runtime::spawn_blocking(move || {
                match nuomi_core::store::Db::open(&seed_db) {
                    Ok(mut db) => {
                        match nuomi_core::services::presets::seed_builtin_roles(&mut db.0) {
                            Ok(report) => tracing::info!(
                                inserted = report.inserted,
                                updated = report.updated,
                                skipped = report.skipped,
                                "preset roles seeded"
                            ),
                            Err(e) => tracing::warn!(error = %e, "preset role seeding failed"),
                        }
                    }
                    Err(e) => tracing::warn!(error = %e, "preset seeding db open failed"),
                }
            });

            if let Err(e) = app.emit(KERNEL_READY_EVENT, ()) {
                tracing::warn!(error = %e, "failed to emit kernel-ready");
            }

            // Provider connection warming: DNS/TLS pre-connect against every
            // enabled base URL. Deliberately off the critical path; failures
            // are logged and never propagate.
            tauri::async_runtime::spawn(async move {
                let warm_started = std::time::Instant::now();
                let results = nuomi_core::providers::warm_from_store(warm_db).await;
                for r in &results {
                    tracing::info!(
                        base_url = %r.base_url,
                        ok = r.ok,
                        latency_ms = r.latency_ms,
                        "provider warm probe"
                    );
                }
                tracing::info!(
                    count = results.len(),
                    elapsed_ms = warm_started.elapsed().as_millis() as u64,
                    "provider warming finished"
                );
            });

            tracing::info!(
                boot_ms,
                ready_ms = started.elapsed().as_millis() as u64,
                "kernel ready (window shown independently of boot)"
            );
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "kernel boot failed"
            );
            let _ = app.emit(KERNEL_FAILED_EVENT, e.to_string());
        }
    }
}

pub fn run() {
    init_tracing();
    let builder = specta_builder();
    #[cfg(debug_assertions)]
    if let Err(e) = export_bindings() {
        eprintln!("bindings export failed: {e}");
    }

    tauri::Builder::default()
        // OS folder picker for the 切换工作区 dialog (WorkspaceDialog.tsx).
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            let db_path = std::env::var_os("NUOMI_DB_PATH")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| {
                    app.path()
                        .app_data_dir()
                        .unwrap_or_else(|_| std::path::PathBuf::from("."))
                        .join("nuomi.db")
                });
            // Editor sandbox defaults under the app data dir — never the
            // process cwd (see `AppState::boot_with_secrets` docs).
            let default_workspace = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("sandbox");
            let _ = std::fs::create_dir_all(&default_workspace);
            let api_key = std::env::var("NUOMI_API_KEY").unwrap_or_default();
            let base_url = std::env::var("NUOMI_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
            let model = std::env::var("NUOMI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".to_string());
            let provider = nuomi_core::facade::ProviderSource::Endpoint(
                nuomi_core::facade::ProviderEndpoint {
                    protocol: nuomi_core::domain::ProviderProtocol::OpenAiCompatible,
                    base_url,
                    api_key,
                    model,
                },
            );

            // Kernel boot moved OFF the setup hook: the setup hook runs on
            // the main thread before the event loop pumps, so any blocking
            // here (SQLite migrations, fs IO) delays window paint for the
            // whole duration. Only cheap path computation stays synchronous;
            // everything else happens in `boot_and_wire` on the async
            // runtime while the window appears immediately.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(boot_and_wire(
                handle,
                db_path,
                provider,
                default_workspace,
            ));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running nuomi shell");
}
