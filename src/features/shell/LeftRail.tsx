import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useUiStore, type View, PANEL_MIN_WIDTH, PANEL_MAX_WIDTH, PANEL_DEFAULT_WIDTH } from "../../lib/store/uiStore";
import { Icon, type IconName } from "../../components/ui/Icon/Icon";
import { PanelResizer } from "../../components/ui/PanelResizer";
import { ConversationsList } from "../conversation/ConversationsList";
import { ThemeToggle } from "./ThemeToggle";
import { LeftPanelDock } from "./LeftPanelDock";

export const NAV_ITEMS: Array<{ view: View; labelKey: string; icon: IconName }> = [
  { view: "chat", labelKey: "shell.navChat", icon: "chat" },
  { view: "steward", labelKey: "shell.navSteward", icon: "sparkles" },
  { view: "board", labelKey: "shell.navBoard", icon: "board" },
  { view: "trace", labelKey: "shell.navTrace", icon: "trace" },
  { view: "git", labelKey: "shell.navGit", icon: "git" },
  { view: "sandbox", labelKey: "shell.navSandbox", icon: "play" },
  { view: "approvals", labelKey: "shell.navApprovals", icon: "approvals" },
  { view: "scheduler", labelKey: "shell.navScheduler", icon: "scheduler" },
  { view: "plugins", labelKey: "shell.navPlugins", icon: "plugins" },
  { view: "settings", labelKey: "shell.navSettings", icon: "settings" },
];

/**
 * Left rail: view switcher on top, session list below. Three layout states
 * (uiStore.leftPanel, persisted): expanded (resizable width + edge resizer),
 * collapsed (LeftPanelDock icon column), or fully hidden (renders nothing —
 * restored via the AreaNav toggle).
 */
export function LeftRail(): ReactNode {
  const view = useUiStore((s) => s.view);
  const setView = useUiStore((s) => s.setView);
  const leftPanel = useUiStore((s) => s.leftPanel);
  const setLeftPanel = useUiStore((s) => s.setLeftPanel);
  const { t } = useTranslation();

  if (leftPanel.hidden) return null;
  if (leftPanel.collapsed) return <LeftPanelDock />;

  return (
    <nav
      aria-label={t("shell.appName")}
      style={{ width: leftPanel.width }}
      className="relative flex shrink-0 flex-col border-r border-ink-muted/30 bg-surface-raised"
    >
      <span aria-hidden="true" className="binder-holes pointer-events-none absolute inset-y-0 left-0 w-3" />
      <div className="flex flex-col py-2 pl-5 pr-2">
        {NAV_ITEMS.map((item) => (
          <button
            key={item.view}
            type="button"
            data-view={item.view}
            onClick={() => {
              // setView already releases the editor area (see uiStore).
              setView(item.view);
            }}
            aria-current={view === item.view ? "page" : undefined}
            className={`flex items-center gap-2 rounded-[12px_255px_15px_225px/225px_15px_255px_12px] px-3 py-1.5 text-left font-note-hand text-sm focus-visible:ring-2 focus-visible:ring-ink-accent ${
              view === item.view
                ? "bg-surface-overlay text-ink-accent ring-1 ring-inset ring-ink-muted/40"
                : "text-ink-muted hover:bg-surface-overlay"
            }`}
          >
            <Icon name={item.icon} size={16} className="nav-icon shrink-0" />
            {t(item.labelKey)}
          </button>
        ))}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto border-t border-ink-muted/30">
        <ConversationsList />
      </div>
      <div className="shrink-0 border-t border-ink-muted/30 p-1">
        <ThemeToggle />
      </div>
      <PanelResizer
        side="left"
        width={leftPanel.width}
        minWidth={PANEL_MIN_WIDTH}
        maxWidth={PANEL_MAX_WIDTH}
        onWidthChange={(width) => setLeftPanel({ width })}
        onCollapse={() => setLeftPanel({ collapsed: true })}
        onReset={() => setLeftPanel({ width: PANEL_DEFAULT_WIDTH })}
        resizeLabel={t("shell.panelResize")}
        collapseLabel={t("shell.panelCollapse")}
      />
    </nav>
  );
}
