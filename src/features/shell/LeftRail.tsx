import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useUiStore, type View } from "../../lib/store/uiStore";
import { SessionsList } from "./SessionsList";
import { ThemeToggle } from "./ThemeToggle";

const NAV_ITEMS: Array<{ view: View; labelKey: string }> = [
  { view: "chat", labelKey: "shell.navChat" },
  { view: "board", labelKey: "shell.navBoard" },
  { view: "trace", labelKey: "shell.navTrace" },
  { view: "git", labelKey: "shell.navGit" },
  { view: "approvals", labelKey: "shell.navApprovals" },
  { view: "scheduler", labelKey: "shell.navScheduler" },
  { view: "settings", labelKey: "shell.navSettings" },
];

/** Left rail: view switcher on top, session list below. */
export function LeftRail(): ReactNode {
  const view = useUiStore((s) => s.view);
  const setView = useUiStore((s) => s.setView);
  // Selecting a left-rail view reveals the chat side of the main area (the
  // editor may be hiding it after a file was opened).
  const setActiveArea = useUiStore((s) => s.setActiveArea);
  const { t } = useTranslation();
  return (
    <nav aria-label={t("shell.appName")} className="relative flex w-56 shrink-0 flex-col border-r border-ink-muted/30 bg-surface-raised">
      <span aria-hidden="true" className="binder-holes pointer-events-none absolute inset-y-0 left-0 w-3" />
      <div className="flex flex-col py-2 pl-5 pr-2">
        {NAV_ITEMS.map((item) => (
          <button
            key={item.view}
            type="button"
            onClick={() => {
              setView(item.view);
              setActiveArea("chat");
            }}
            aria-current={view === item.view ? "page" : undefined}
            className={`rounded-[12px_255px_15px_225px/225px_15px_255px_12px] px-3 py-1.5 text-left font-note-hand text-sm focus-visible:ring-2 focus-visible:ring-ink-accent ${
              view === item.view
                ? "bg-surface-overlay text-ink-accent ring-1 ring-inset ring-ink-muted/40"
                : "text-ink-muted hover:bg-surface-overlay"
            }`}
          >
            {t(item.labelKey)}
          </button>
        ))}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto border-t border-ink-muted/30">
        <SessionsList />
      </div>
      <div className="shrink-0 border-t border-ink-muted/30 p-1">
        <ThemeToggle />
      </div>
    </nav>
  );
}
