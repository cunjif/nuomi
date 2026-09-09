import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { CliAgentsSection } from "./CliAgentsSection";
import { IntegrationsSection } from "./IntegrationsSection";
import { OnlineAuthToggle } from "./OnlineAuthToggle";
import { ProvidersSection } from "./ProvidersSection";
import { RolesSection } from "./RolesSection";
import { SensitiveToolsEditor } from "./SensitiveToolsEditor";
import { TeamsSection } from "./TeamsSection";

/**
 * Settings tabs (用户需求: 集中一页 → 顶部导航分 tab). One section per tab;
 * only the active panel mounts, so each tab keeps its own short scroll
 * instead of one long page (配合 html/body overflow:hidden，窗口永不滚动).
 */
const TABS = [
  { key: "providers", labelKey: "settings.tab.providers" },
  { key: "roles", labelKey: "settings.roles.heading" },
  { key: "teams", labelKey: "settings.teams.heading" },
  { key: "agents", labelKey: "settings.cliAgents.heading" },
  { key: "integrations", labelKey: "settings.integrations.heading" },
  { key: "access", labelKey: "settings.tab.access" },
] as const;

type TabKey = (typeof TABS)[number]["key"];

function TabPanel({ tab }: { tab: TabKey }): ReactNode {
  switch (tab) {
    case "providers":
      return <ProvidersSection />;
    case "roles":
      return <RolesSection />;
    case "teams":
      return <TeamsSection />;
    case "agents":
      return <CliAgentsSection />;
    case "integrations":
      return <IntegrationsSection />;
    case "access":
      return (
        <>
          <SensitiveToolsEditor />
          <OnlineAuthToggle />
        </>
      );
  }
}

/** U13 settings: tabbed sections (providers / roles / teams / agents / integrations / access). */
export function SettingsView(): ReactNode {
  const { t } = useTranslation();
  const [tab, setTab] = useState<TabKey>("providers");

  return (
    <div className="flex h-full min-h-0 flex-col">
      <nav
        role="tablist"
        aria-label={t("settings.navLabel")}
        className="flex shrink-0 items-center gap-1 overflow-x-auto border-b border-ink-muted/30 px-3 py-1.5"
      >
        {TABS.map((item) => (
          <button
            key={item.key}
            type="button"
            role="tab"
            aria-selected={tab === item.key}
            onClick={() => setTab(item.key)}
            className={`shrink-0 rounded-[12px_255px_15px_225px/225px_15px_255px_12px] px-3 py-1 font-note-hand text-xs focus-visible:ring-2 focus-visible:ring-ink-accent ${
              tab === item.key
                ? "bg-surface-overlay text-ink-accent ring-1 ring-inset ring-ink-muted/40"
                : "text-ink-muted hover:bg-surface-overlay"
            }`}
          >
            {t(item.labelKey)}
          </button>
        ))}
      </nav>
      <div role="tabpanel" className="min-h-0 flex-1 overflow-y-auto p-3">
        <TabPanel tab={tab} />
      </div>
    </div>
  );
}
