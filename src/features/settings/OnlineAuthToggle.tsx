import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

/** Evolution online-learning authorization switch. */
export function OnlineAuthToggle(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const query = useQuery({ queryKey: ["onlineAuthorized"], queryFn: ipc.getOnlineAuthorized });
  const [optimistic, setOptimistic] = useState<boolean | null>(null);

  const setMut = useMutation({
    mutationFn: (authorized: boolean) => ipc.setOnlineAuthorized(authorized),
    onSuccess: () => {
      setOptimistic(null);
      void qc.invalidateQueries({ queryKey: ["onlineAuthorized"] });
    },
    onError: (e) => {
      setOptimistic(null);
      toast.error(`${t("settings.onlineToggleFailed")}: ${describeError(e)}`);
    },
  });

  const checked = optimistic ?? query.data ?? false;

  return (
    <section aria-label={t("settings.onlineHeading")} className="rounded border border-ink-muted/40 bg-surface-raised p-3">
      <h3 className="text-sm font-semibold text-ink">{t("settings.onlineHeading")}</h3>
      <p className="mb-2 text-xs text-ink-muted">{t("settings.onlineDescription")}</p>
      <AsyncBoundary isLoading={query.isLoading} error={query.error} isEmpty={false} onRetry={() => void query.refetch()}>
        <button
          type="button"
          role="switch"
          aria-checked={checked}
          disabled={setMut.isPending}
          onClick={() => {
            setOptimistic(!checked);
            setMut.mutate(!checked);
          }}
          className={`rounded px-3 py-1 text-xs focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50 ${
            checked ? "bg-state-ok/20 text-state-ok" : "bg-surface-overlay text-ink-muted"
          }`}
        >
          {checked ? t("common.enabled") : t("common.disabled")}
        </button>
      </AsyncBoundary>
    </section>
  );
}
