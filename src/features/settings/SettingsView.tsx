import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { ipc } from "../../lib/ipc/client";
import { CliAgentsSection } from "./CliAgentsSection";
import { IntegrationsSection } from "./IntegrationsSection";
import { OnlineAuthToggle } from "./OnlineAuthToggle";
import { ProviderForm } from "./ProviderForm";
import { RolesSection } from "./RolesSection";
import { SensitiveToolsEditor } from "./SensitiveToolsEditor";
import { TeamsSection } from "./TeamsSection";

/** U13 settings: providers + sensitive tools + evolution authorization. */
export function SettingsView(): ReactNode {
  const { t } = useTranslation();
  const [formOpen, setFormOpen] = useState(false);
  const providersQuery = useQuery({ queryKey: ["providers"], queryFn: ipc.listProviders });

  return (
    <div className="h-full overflow-y-auto p-3">
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-sm font-semibold">{t("settings.providersHeading")}</h2>
        <button
          type="button"
          onClick={() => setFormOpen((o) => !o)}
          aria-expanded={formOpen}
          className="rounded bg-ink-accent px-3 py-1 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent"
        >
          {t("common.add")}
        </button>
      </div>
      {formOpen && <ProviderForm onDone={() => setFormOpen(false)} />}
      <div className="mb-3">
        <AsyncBoundary
          isLoading={providersQuery.isLoading}
          error={providersQuery.error}
          isEmpty={(providersQuery.data?.length ?? 0) === 0}
          emptyLabel={t("common.empty")}
          onRetry={() => void providersQuery.refetch()}
        >
          <ul className="flex flex-col gap-2">
            {(providersQuery.data ?? []).map((provider) => (
              <li key={provider.id} className="rounded border border-ink-muted/40 bg-surface-raised p-3 text-xs">
                <p className="text-sm font-medium text-ink">
                  {provider.name}
                  {provider.isMaster && (
                    <span className="ml-2 rounded bg-ink-accent/20 px-1.5 py-0.5 text-[10px] text-ink-accent">
                      {t("settings.masterTag")}
                    </span>
                  )}
                </p>
                <p className="mt-0.5 font-mono text-ink-muted">{provider.baseUrl}</p>
                <p className="mt-0.5 text-ink-muted">
                  {provider.protocol} ·{" "}
                  {provider.hasKey ? t("settings.hasKey") : t("settings.noKey")} · {provider.capabilities.join(", ")}
                </p>
              </li>
            ))}
          </ul>
        </AsyncBoundary>
      </div>
      <SensitiveToolsEditor />
      <CliAgentsSection />
      <RolesSection />
      <TeamsSection />
      <IntegrationsSection />
      <OnlineAuthToggle />
    </div>
  );
}
