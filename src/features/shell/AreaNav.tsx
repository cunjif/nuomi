import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useUiStore, type ActiveArea } from "../../lib/store/uiStore";

const AREA_TABS: Array<{ area: ActiveArea; labelKey: string; icon: ReactNode }> = [
  {
    area: "chat",
    labelKey: "nav.chat",
    icon: (
      <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true" fill="currentColor">
        <path d="M2 2h12a1 1 0 0 1 1 1v7a1 1 0 0 1-1 1H6l-3.6 3a.5.5 0 0 1-.8-.4V3a1 1 0 0 1 .4-.9Z" />
      </svg>
    ),
  },
  {
    area: "editor",
    labelKey: "nav.editor",
    icon: (
      <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true" fill="currentColor">
        <path d="M3 1h7l4 4v10a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V2a1 1 0 0 1 1-1Zm7 1v3h3Z" />
      </svg>
    ),
  },
];

/**
 * Central main-area navigation (需求 5): 对话 | 文件编辑 toggle. The two
 * surfaces are mutually exclusive — clicking a tab (or opening a workspace
 * file) flips uiStore.activeArea.
 *
 * `right` hosts an optional context toolbar slot (VSCode-style: the editor
 * workspace actions share the nav strip, right-aligned) without disturbing
 * the centered tabs.
 */
export function AreaNav({ right }: { right?: ReactNode }): ReactNode {
  const { t } = useTranslation();
  const activeArea = useUiStore((s) => s.activeArea);
  const setActiveArea = useUiStore((s) => s.setActiveArea);
  return (
    <div
      role="tablist"
      aria-label={t("nav.areaLabel")}
      className="relative flex h-9 shrink-0 items-center justify-center gap-1 border-b border-ink-muted/30 bg-surface-raised px-3"
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
      {right !== undefined && (
        <div className="absolute inset-y-0 right-2 flex items-center gap-1">{right}</div>
      )}
    </div>
  );
}
