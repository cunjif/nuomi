import { useState } from "react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { StewardChat } from "./StewardChat";
import { CycleProgress } from "./CycleProgress";
import { GateInbox } from "./GateInbox";
import { TeamManagement } from "./TeamManagement";
import { SettingsView as StewardSettingsView } from "./SettingsView";

type StewardTab = "chat" | "cycles" | "gate" | "team" | "settings";

const TABS: Array<{ tab: StewardTab; labelKey: string }> = [
  { tab: "chat", labelKey: "steward.tabChat" },
  { tab: "cycles", labelKey: "steward.tabCycles" },
  { tab: "gate", labelKey: "steward.tabGate" },
  { tab: "team", labelKey: "steward.tabTeam" },
  { tab: "settings", labelKey: "steward.tabSettings" },
];

/** Container for the App Steward AI surface with sub-tab navigation. */
export function StewardView(): ReactNode {
  const [tab, setTab] = useState<StewardTab>("chat");
  const { t } = useTranslation();
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex shrink-0 items-center gap-1 border-b border-ink-muted/30 px-3 py-2">
        {TABS.map((item) => (
          <button
            key={item.tab}
            type="button"
            onClick={() => setTab(item.tab)}
            aria-current={tab === item.tab ? "page" : undefined}
            className={`rounded-md px-3 py-1 text-sm font-medium transition-colors ${
              tab === item.tab
                ? "bg-surface-overlay text-ink-accent"
                : "text-ink-muted hover:bg-surface-overlay hover:text-ink"
            }`}
          >
            {t(item.labelKey)}
          </button>
        ))}
      </div>
      <div className="min-h-0 flex-1 overflow-hidden">
        {tab === "chat" && <StewardChat />}
        {tab === "cycles" && <CycleProgress />}
        {tab === "gate" && <GateInbox />}
        {tab === "team" && <TeamManagement />}
        {tab === "settings" && <StewardSettingsView />}
      </div>
    </div>
  );
}
