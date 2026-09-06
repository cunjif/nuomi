# Startup Performance Analysis — Desktop Shell (Window → Usable UI)

Date: 2026-09-06 · Scope: `src-tauri/src/lib.rs` boot chain + frontend `main.tsx → App → Shell` gating.

## 1. Boot timeline (before the fix)

```
process start
  └─ tauri::Builder::run
      └─ setup hook (MAIN THREAD, before event loop pumps messages)
          ├─ db_path / default_workspace computation     (~0 ms, pure path math)
          ├─ block_on(AppState::boot)                    ← ★ WINDOW BLOCKED HERE
          │   ├─ NuomiKernel::boot
          │   │   └─ spawn_blocking: Db::open + migrations 0001–0008
          │   │      (incl. 0005 FTS5, 0006 CDC triggers) + first session insert
          │   ├─ provider construction (no IO)            (~0 ms)
          │   ├─ plugin/service registration (in-memory)  (~0 ms)
          │   └─ workspace settings read (second Db::open, depends on migrations)
          ├─ block_on { SchedulerRunner::spawn, notifier::spawn }
          └─ app.manage(state)                           ← window paints only after this
```

Structural root cause: **`block_on(AppState::boot)` inside the setup hook**. The setup hook
runs on the main thread before the event loop starts, so the config-defined window existed
but could not paint/respond until the entire kernel boot finished. Any stall on the boot
path (AV scan, slow FS, large DB) directly translated into "no window for N minutes".

## 2. Measured phase data (this machine, Windows 11)

Instrumentation added (tracing `info` spans, visible with `RUST_LOG=info`):

| Phase | Instrument | Measured |
|---|---|---|
| `Db::open` + migrations 0001–0008 + first session, **copy of the real DB (244 KB)** | `facade.rs db_boot_ms` | **10–11 ms** |
| Same, **fresh DB, cold run** (all migrations incl. FTS5/CDC created, OS cache cold) | `facade.rs db_boot_ms` | **180 ms** |
| Same, fresh DB, warm run | `facade.rs db_boot_ms` | **12 ms** |
| Provider construction + plugin registration (in-memory) | code inspection | ~0 ms (no IO) |
| Keyring (`OsKeyring`) | code inspection (`state.rs`) | **0 IO on boot path** — only `Arc::new(OsKeyring)` construction; credentials are read lazily per-command |
| Workspace settings read | `state.rs workspace_resolve_ms` | single indexed lookup on fresh connection; depends on migrations having run (table `app_settings` from 0008) |

Measurement method: `cargo run -p nuomi-cli -- run "ping" --db <copy of real DB>` with
`RUST_LOG=info` (the CLI shares `NuomiKernel::boot` with the shell; the real DB was copied,
never touched). **实测** for the DB phases; the full GUI window-paint-to-visible time could
not be reproduced in this environment (no interactive desktop session) — **推断** where noted.

## 3. Root causes, ranked

1. **[STRUCTURAL — fixed] Synchronous `block_on(AppState::boot)` in the setup hook.**
   The window is config-defined (created before setup), but the main thread was blocked,
   so paint/event-loop start waited for the whole boot. This turns *any* boot-path stall
   into "no usable window". Fix: boot moved to `tauri::async_runtime::spawn`
   (`boot_and_wire`); setup now only does path math and returns.
2. **[ENVIRONMENTAL — inferred, cannot reproduce here] The 10-minute magnitude is not
   explainable by the kernel boot itself (10–180 ms measured).** The realistic sources of
   a minute-scale Windows startup are outside `AppState::boot`:
   - `tauri dev` full pipeline: Rust rebuild + `pnpm dev` (Vite) + WebView2 navigation —
     a dev-loop cost, **not** production app startup; must be excluded from the metric.
   - Antivirus/Defender first-touch scanning of freshly built binaries and the `target/`
     tree (well documented to add minutes on Windows dev machines).
   - WebView2 runtime cold start / policy checks on locked-down or domain machines.
   With boot off the critical path these now degrade gracefully: the window and the boot
   screen appear immediately, and the UI becomes usable as soon as the kernel is ready.
3. **[FRONTEND — fixed] Serialized first content.** `Shell` rendered a bare spinner until
   `getWorkspace` completed; every child view then fired its own IPC queries serially.
   Fix: kernel-ready event + retry-poll unblocks the shell; last validated workspace is
   cached in `localStorage` and used as `placeholderData` (stale-while-revalidate) so the
   main layout paints immediately after kernel-ready while validation runs in background.

## 4. Changes made

- `src-tauri/src/lib.rs`: setup hook no longer blocks; `boot_and_wire` spawns boot, event
  bridge, `SchedulerRunner`, notifier, manages `AppState`, emits `kernel-ready`
  (or `kernel-failed`), then kicks off `providers::warm_from_store` as a detached task
  (DNS/TLS pre-connect; results logged, never propagated).
- `src-tauri/src/state.rs`, `crates/nuomi-core/src/facade.rs`: phase timings kept as
  permanent `tracing::info!` one-liners (`kernel_boot_ms`, `workspace_resolve_ms`,
  `total_ms`, `db_boot_ms`) plus a final summary on the shell side.
- Boot internals were audited for parallelization: the only IO region is the single
  `spawn_blocking` DB phase; the settings read genuinely depends on migrations (first-boot
  race if parallelized), and keyring/services do no IO. Parallelizing would add risk for
  ~0 ms gain — deliberately kept serial (correctness first).
- `src/features/shell/Shell.tsx`: boot screen while kernel boots; listens for
  `kernel-ready`/`kernel-failed`; workspace query polls through the "state not managed"
  window (retry every 500 ms) as a fallback for a missed event; `localStorage`-cached
  workspace as `placeholderData` for progressive first paint.

## 5. Dev vs production mode

| Mode | Startup composition | Notes |
|---|---|---|
| `tauri dev` | Rust incremental build → `pnpm dev` (Vite) → app boot → WebView2 load of `http://localhost:1420` | Build + Vite dominate; **not** representative of app startup |
| `pnpm tauri build` output | process start → app boot (measured 10–180 ms) → window paint → kernel-ready | True startup metric; verify with `RUST_LOG=info` tracing lines |

## 6. Verification

- `cargo fmt --all` clean; `cargo clippy -p nuomi-core -p nuomi-shell --all-targets -D warnings` clean.
- `cargo test -p nuomi-core -p nuomi-shell` all green (incl. the 5 src-tauri integration test files).
- `pnpm typecheck && pnpm lint && pnpm test` green.
