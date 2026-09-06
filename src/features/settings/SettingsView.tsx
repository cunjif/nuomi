import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { CliAgentsSection } from "./CliAgentsSection";
import { IntegrationsSection } from "./IntegrationsSection";
import { OnlineAuthToggle } from "./OnlineAuthToggle";
import { ProvidersSection } from "./ProvidersSection";
import { RolesSection } from "./RolesSection";
import { SensitiveToolsEditor } from "./SensitiveToolsEditor";
import { TeamsSection } from "./TeamsSection";

/** U13 settings: providers + sensitive tools + evolution authorization. */
export function SettingsView(): ReactNode {
  const { t } = useTranslation();

  return (
    <div className="h-full overflow-y-auto p-3">
      <h2 className="mb-2 text-sm font-semibold">{t("settings.providersHeading")}</h2>
      <ProvidersSection />
      <SensitiveToolsEditor />
      <CliAgentsSection />
      <RolesSection />
      <TeamsSection />
      <IntegrationsSection />
      <OnlineAuthToggle />
    </div>
  );
}
