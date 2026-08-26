import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { IntegrationKindDto } from "../../lib/ipc/bindings.gen";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { IntegrationForm } from "./IntegrationForm";

const KIND_LABEL_KEYS: Record<IntegrationKindDto, string> = {
  feishu_bot: "settings.integrations.kindFeishuBot",
  qq_webhook: "settings.integrations.kindQqWebhook",
  telemetry: "settings.integrations.kindTelemetry",
};

/** Settings section listing integrations with test/delete actions; edits hot-reload the notifier. */
export function IntegrationsSection(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const query = useQuery({ queryKey: ["integrations"], queryFn: ipc.listIntegrations });
  /** integrationId awaiting a second click on Delete (two-step confirm, keyboard friendly) */
  const [confirmingId, setConfirmingId] = useState<string | null>(null);

  const testMut = useMutation({
    mutationFn: (integrationId: string) => ipc.testIntegration(integrationId),
    onSuccess: (result) => {
      if (result.ok) {
        toast.success(t("settings.integrations.testOk"));
      } else if (result.error !== null) {
        toast.error(`${t("settings.integrations.testFailed")}: ${result.error}`);
      }
    },
    onError: (e) => toast.error(`${t("settings.integrations.testFailed")}: ${describeError(e)}`),
  });

  const deleteMut = useMutation({
    mutationFn: (integrationId: string) => ipc.deleteIntegration(integrationId),
    onSuccess: () => {
      setConfirmingId(null);
      void qc.invalidateQueries({ queryKey: ["integrations"] });
      toast.success(t("settings.integrations.deleted"));
    },
    onError: (e) => toast.error(`${t("settings.integrations.deletedFailed")}: ${describeError(e)}`),
  });

  return (
    <section aria-label={t("settings.integrations.heading")} className="mb-3">
      <h3 className="mb-2 text-sm font-semibold text-ink">{t("settings.integrations.heading")}</h3>
      <IntegrationForm />
      <AsyncBoundary
        isLoading={query.isLoading}
        error={query.error}
        isEmpty={(query.data?.length ?? 0) === 0}
        emptyLabel={t("settings.integrations.empty")}
        onRetry={() => void query.refetch()}
      >
        <ul className="flex flex-col gap-2">
          {(query.data ?? []).map((integration) => (
            <li key={integration.id} className="rounded border border-ink-muted/40 bg-surface-raised p-3 text-xs">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-sm font-medium text-ink">{integration.name}</span>
                <span className="rounded bg-ink-accent/20 px-1.5 py-0.5 text-[10px] text-ink-accent">
                  {t(KIND_LABEL_KEYS[integration.kind])}
                </span>
                <span className={integration.enabled ? "text-state-ok" : "text-ink-muted"}>
                  {integration.enabled ? t("common.enabled") : t("common.disabled")}
                </span>
              </div>
              <p className="mt-0.5 font-mono text-ink-muted">{integration.webhookUrlMasked}</p>
              <div className="mt-1.5 flex gap-2">
                <button
                  type="button"
                  onClick={() => testMut.mutate(integration.id)}
                  disabled={testMut.isPending}
                  aria-label={`${t("settings.integrations.test")} ${integration.name}`}
                  className="rounded border border-ink-muted px-2 py-0.5 text-xs text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
                >
                  {testMut.isPending ? t("settings.integrations.testing") : t("settings.integrations.test")}
                </button>
                {confirmingId === integration.id ? (
                  <button
                    type="button"
                    onClick={() => deleteMut.mutate(integration.id)}
                    disabled={deleteMut.isPending}
                    aria-label={`${t("settings.integrations.deleteConfirm")} ${integration.name}`}
                    className="rounded border border-state-danger px-2 py-0.5 text-xs text-state-danger hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
                  >
                    {t("settings.integrations.deleteConfirm")}
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={() => setConfirmingId(integration.id)}
                    aria-label={`${t("common.delete")} ${integration.name}`}
                    className="rounded border border-ink-muted px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
                  >
                    {t("common.delete")}
                  </button>
                )}
              </div>
            </li>
          ))}
        </ul>
      </AsyncBoundary>
    </section>
  );
}
