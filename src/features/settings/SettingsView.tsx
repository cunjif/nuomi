import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useUiStore } from "../../lib/store/uiStore";
import { CliAgentsSection } from "./CliAgentsSection";
import { EvolutionSettingsPanel } from "./EvolutionSettingsPanel";
import { IdleProposalSection } from "./IdleProposalSection";
import { IntegrationsSection } from "./IntegrationsSection";
import { MemorySection } from "./MemorySection";
import { ProvidersSection } from "./ProvidersSection";
import { RolesSection } from "./RolesSection";
import { SandboxSection } from "./SandboxSection";
import { WorkbenchSection } from "./WorkbenchSection";

/**
 * Settings tabs (用户需求: 集中一页 → 顶部导航分 tab). One section per tab;
 * only the active panel mounts, so each tab keeps its own short scroll
 * instead of one long page (配合 html/body overflow:hidden，窗口永不滚动).
 * The multi-workspace panel is retired with ADR-0021 (task 6.8) — the
 * single-open model leaves nothing user-configurable there.
 */
const TABS = [
  { key: "providers", labelKey: "settings.tab.providers" },
  { key: "roles", labelKey: "settings.roles.heading" },
  { key: "agents", labelKey: "settings.cliAgents.heading" },
  { key: "integrations", labelKey: "settings.integrations.heading" },
  { key: "memory", labelKey: "settings.memory.heading" },
  { key: "access", labelKey: "settings.tab.evolution" },
  { key: "sandbox", labelKey: "settings.tab.sandbox" },
  { key: "workbench", labelKey: "workbench.settings.heading" },
] as const;

type TabKey = (typeof TABS)[number]["key"];

function TabPanel({ tab }: { tab: TabKey }): ReactNode {
  switch (tab) {
    case "providers":
      return <ProvidersSection />;
    case "roles":
      return <RolesSection />;
    case "agents":
      return <CliAgentsSection />;
    case "integrations":
      return <IntegrationsSection />;
    case "memory":
      return <MemorySection />;
    case "access":
      return (
        <>
          <EvolutionSettingsPanel />
          <IdleProposalSection />
        </>
      );
    case "sandbox":
      return <SandboxSection />;
    case "workbench":
      return <WorkbenchSection />;
  }
}

/** U13 settings: tabbed sections (providers / roles / agents / integrations / access). */
export function SettingsView({ initialTab }: { initialTab?: string }): ReactNode {
  const { t } = useTranslation();
  const storeTab = useUiStore((s) => s.settingsInitialTab);
  const setSettingsInitialTab = useUiStore((s) => s.setSettingsInitialTab);
  const effectiveInitial = initialTab ?? storeTab ?? "providers";
  const [tab, setTab] = useState<TabKey>(
    TABS.some((item) => item.key === effectiveInitial) ? (effectiveInitial as TabKey) : "providers",
  );
  // Consume the pending tab so the next manual open starts fresh.
  useEffect(() => {
    if (storeTab !== null) setSettingsInitialTab(null);
  }, [storeTab, setSettingsInitialTab]);

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
