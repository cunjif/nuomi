import type { ReactNode } from "react";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { toast } from "../../lib/store/toastStore";
import { useUiStore, type ActiveArea } from "../../lib/store/uiStore";
import { Icon } from "../../components/ui/Icon/Icon";

const AREA_TABS: Array<{ area: ActiveArea; labelKey: string; icon: ReactNode }> = [
  { area: "chat", labelKey: "nav.chat", icon: <Icon name="chat" size={12} /> },
  { area: "workbench", labelKey: "nav.workbench", icon: <Icon name="file" size={12} /> },
];

/** True inside a real Tauri webview; jsdom / plain browser hides controls. */
function hasTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}


/**
 * Window control buttons (minimize / maximize / close) for the custom
 * title bar — only rendered under the Tauri runtime. Buttons must NOT
 * carry data-tauri-drag-region (drag region lives on the row around them).
 */
function WindowControls(): ReactNode {
  const { t } = useTranslation();
  const win = useMemo(() => getCurrentWindow(), []);
  // Surface ACL/runtime failures — a silently dead button is undebuggable
  // (missing capability permissions reject the promise).
  const run = (action: () => Promise<void>): void => {
    action().catch((e: unknown) => toast.error(`${t("shell.windowControlFailed")}: ${String(e)}`));
  };
  const base =
    "flex h-9 w-11 items-center justify-center text-xs text-ink-muted hover:bg-surface-overlay hover:text-ink focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ink-accent";
  return (
    <div className="relative z-10 ml-auto flex items-stretch self-stretch">
      <button type="button" aria-label={t("shell.minimize")} onClick={() => run(() => win.minimize())} className={base}>
        <Icon name="minimize" size={14} />
      </button>
      <button type="button" aria-label={t("shell.maximize")} onClick={() => run(() => win.toggleMaximize())} className={base}>
        <Icon name="maximize" size={14} />
      </button>
      <button
        type="button"
        aria-label={t("shell.close")}
        onClick={() => run(() => win.close())}
        className={`${base} hover:bg-state-danger hover:text-surface`}
      >
        <Icon name="close" size={14} />
      </button>
    </div>
  );
}

/**
 * Custom window title bar merged with the area nav (用户 ASCII 布局): the
 * native frame is disabled (tauri.conf.json decorations:false), so this
 * header owns dragging, the window buttons and everything the old top strip
 * had.
 *
 * Dragging: the Tauri window plugin injects a document-level mousedown
 * handler (tauri/src/window/scripts/drag.js) that walks the event path for
 * `data-tauri-drag-region`. Values: bare/"true" = only direct hits on that
 * element, "deep" = any non-clickable descendant (BUTTON/A/INPUT/label/
 * summary/[role=tab|button|…] still block it), "false" = opt out. This row
 * needs "deep": the absolutely-positioned tab list covers it, so a bare
 * attribute would almost never be the direct mousedown target and the window
 * would not move. Double-click → internal_toggle_maximize is handled by the
 * same script. Requires capability core:window:allow-start-dragging.
 *
 *   row 1: [N] nuomi · 糯米   [对话] [文件编辑]   — ▢ ✕
 *   row 2:                                    (●) 已连接
 *
 * The two main surfaces are mutually exclusive — clicking a tab (or opening
 * a workspace file) flips uiStore.activeArea. Under vitest / plain browser
 * the window buttons are simply absent and the strip behaves as before.
 */
export function AreaNav(): ReactNode {
  const { t } = useTranslation();
  const activeArea = useUiStore((s) => s.activeArea);
  const setActiveArea = useUiStore((s) => s.setActiveArea);
  const tauriReady = hasTauriRuntime();

  return (
    <header className="shrink-0 select-none border-b border-ink-muted/30 bg-surface-raised">
      {/* "deep" = every non-clickable descendant drags (tabs / window buttons
      stay clickable). A bare attr would mean "this element only". */}
      <div data-tauri-drag-region="deep" className="relative flex h-9 items-center pl-2 pr-0">
        {/* Brand + app icon (part of the drag region). */}
        <div className="flex items-center gap-1.5">
          <span
            aria-hidden="true"
            className="flex size-5 items-center justify-center rounded-[155px_12px_155px_12px/12px_155px_12px_155px] border border-dashed border-ink-muted/50 bg-surface-overlay text-[10px] font-bold text-ink-accent"
          >
            N
          </span>
          <span className="text-title-hand text-xs font-semibold">{t("shell.appTitle")}</span>
        </div>
        {/* Area tabs centered on the title row. */}
        <div
          role="tablist"
          aria-label={t("nav.areaLabel")}
          className="absolute inset-x-0 flex justify-center gap-1"
        >
          {AREA_TABS.map((tab) => (
            <button
              key={tab.area}
              type="button"
              role="tab"
              aria-selected={activeArea === tab.area}
              onClick={() => setActiveArea(tab.area)}
              className={`flex items-center gap-1.5 rounded px-3 py-1 text-xs focus-visible:ring-2 focus-visible:ring-ink-accent ${
                activeArea === tab.area ? "bg-surface-overlay text-ink-accent" : "text-ink-muted hover:bg-surface-overlay"
              }`}
            >
              {tab.icon}
              {t(tab.labelKey)}
            </button>
          ))}
        </div>
        {/* Window controls right (Tauri only); drag filler keeps ml-auto
        spacing when controls are hidden (browser/tests). */}
        {tauriReady ? (
          <WindowControls />
        ) : (
          <span className="ml-auto h-full w-8" aria-hidden="true" />
        )}
      </div>
    </header>
  );
}
