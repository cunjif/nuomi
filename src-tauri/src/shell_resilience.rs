//! Shell resilience: OS-level fallbacks for when the webview white-screens.
//!
//! The custom title bar (`decorations: false`) delegates all window control
//! to the frontend DOM — `data-tauri-drag-region` for dragging, React context
//! menus for closing. When the webview crashes or fails to mount, the user is
//! locked out of a blank window with no way to drag, close, or recover.
//!
//! This module installs three OS-level safety nets that do not depend on the
//! webview being alive:
//!
//! - **System tray** with reload/show/hide/quit menu (always reachable from
//!   the taskbar / menu bar).
//! - **Global Ctrl+Shift+R** shortcut that injects `location.reload()` from
//!   the Rust side (a browser primitive that works even when the JS context
//!   is wedged).
//! - **Watchdog** that auto-reloads if the frontend never sends a heartbeat
//!   within a grace period, up to a bounded number of retries.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};

/// Grace period before the watchdog assumes the webview failed to mount.
const WATCHDOG_GRACE: Duration = Duration::from_secs(15);
/// Maximum number of auto-reload attempts before the watchdog gives up and
/// emits `shell-load-failed`.
const MAX_RELOAD_ATTEMPTS: u32 = 3;

/// Watchdog state managed into Tauri. The frontend pings
/// `__nuomi_heartbeat` once it has mounted; the watchdog checks this flag
/// after the grace period and reloads if it is still false.
#[derive(Default)]
pub struct WatchdogState {
    ready: AtomicBool,
    attempts: AtomicU32,
}

impl WatchdogState {
    pub fn mark_ready(&self) {
        self.ready.store(true, Ordering::Relaxed);
    }

    fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }

    fn bump_attempts(&self) -> u32 {
        self.attempts.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// Installs the tray icon, global shortcut, and watchdog. Call from the
/// Tauri `setup` hook with the app handle.
pub fn install(app: AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    app.manage(WatchdogState::default());
    install_tray(&app)?;
    install_global_shortcut(&app)?;
    spawn_watchdog(app);
    Ok(())
}

/// Forces a full webview reload by injecting `location.reload()` from the
/// Rust side. This is a browser primitive that works even when the JS
/// context is wedged (unlike an IPC call, which needs the frontend alive).
fn reload_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        match window.eval("location.reload()") {
            Ok(()) => tracing::info!("main webview reload triggered"),
            Err(e) => tracing::warn!(error = %e, "main webview reload failed"),
        }
    } else {
        tracing::warn!("main webview not found for reload");
    }
}

fn install_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let reload_i = MenuItem::with_id(app, "reload", "重新加载 (Ctrl+Shift+R)", true, None::<&str>)?;
    let show_i = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
    let hide_i = MenuItem::with_id(app, "hide", "隐藏窗口", true, None::<&str>)?;
    let sep_i = PredefinedMenuItem::separator(app)?;
    let quit_i = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&reload_i, &show_i, &hide_i, &sep_i, &quit_i])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| "default window icon missing".to_string())?;

    TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .tooltip("nuomi · 糯米")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "reload" => reload_main_window(app),
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.unminimize();
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "hide" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    if w.is_visible().unwrap_or(false) {
                        let _ = w.hide();
                    } else {
                        let _ = w.unminimize();
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            }
        })
        .build(app)?;
    Ok(())
}

fn install_global_shortcut(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

    let reload = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyR);
    let reload_for_handler = reload;
    app.plugin(
        tauri_plugin_global_shortcut::Builder::new()
            .with_handler(move |app, shortcut, event| {
                if shortcut == &reload_for_handler && event.state() == ShortcutState::Pressed {
                    reload_main_window(app);
                }
            })
            .build(),
    )?;
    app.global_shortcut().register(reload)?;
    Ok(())
}

fn spawn_watchdog(app: AppHandle) {
    let app_for_recurse = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(WATCHDOG_GRACE).await;
        let state = app.state::<WatchdogState>();
        if state.is_ready() {
            return;
        }
        let attempt = state.bump_attempts();
        if attempt > MAX_RELOAD_ATTEMPTS {
            tracing::error!(
                attempts = attempt,
                "webview failed to signal readiness after max reload attempts; giving up"
            );
            let _ = app.emit("shell-load-failed", ());
            return;
        }
        tracing::warn!(attempt, "webview not ready after grace period; auto-reloading");
        reload_main_window(&app);
        // Re-arm for the next attempt.
        spawn_watchdog(app_for_recurse);
    });
}
