import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useUiStore, type WorkbenchSubTab } from "../../lib/store/uiStore";
import { Icon } from "../../components/ui/Icon/Icon";

const SUB_TABS: Array<{ sub: WorkbenchSubTab; labelKey: string; icon: ReactNode }> = [
  { sub: "workspaceList", labelKey: "workbench.workspaceList", icon: <Icon name="grid" size={12} /> },
  { sub: "editor", labelKey: "workbench.editor", icon: <Icon name="file" size={12} /> },
];

/**
 * Second-level navigation within the workbench area: switches between the
 * workspace list panel and the single-workspace file editor. Rendered only
 * while `activeArea === "workbench"` (Shell.tsx routing).
 */
export function WorkbenchNav(): ReactNode {
  const { t } = useTranslation();
  const workbenchSubTab = useUiStore((s) => s.workbenchSubTab);
  const setWorkbenchSubTab = useUiStore((s) => s.setWorkbenchSubTab);

  return (
    <nav
      role="tablist"
      aria-label={t("workbench.subTabLabel")}
      className="flex shrink-0 items-center gap-1 border-b border-ink-muted/30 bg-surface-raised px-2 py-1"
    >
      {SUB_TABS.map((tab) => (
        <button
          key={tab.sub}
          type="button"
          role="tab"
          aria-selected={workbenchSubTab === tab.sub}
          onClick={() => setWorkbenchSubTab(tab.sub)}
          className={`flex items-center gap-1.5 rounded px-2.5 py-1 text-xs focus-visible:ring-2 focus-visible:ring-ink-accent ${
            workbenchSubTab === tab.sub
              ? "bg-surface-overlay text-ink-accent"
              : "text-ink-muted hover:bg-surface-overlay"
          }`}
        >
          {tab.icon}
          {t(tab.labelKey)}
        </button>
      ))}
    </nav>
  );
}
