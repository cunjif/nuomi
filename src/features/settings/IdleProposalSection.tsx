import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

/**
 * Idle team-proposal settings (ADR 0023). Only two of the four backend keys
 * are user-facing (enabled + timeout); backlog threshold and cooldown stay
 * key-documented defaults. Changes save immediately, evolution-panel style.
 */
export function IdleProposalSection(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();

  const settingsQuery = useQuery({
    queryKey: ["idleProposalSettings"],
    queryFn: ipc.getIdleProposalSettings,
  });

  const setMut = useMutation({
    mutationFn: (input: { enabled: boolean; timeoutMinutes: number }) =>
      ipc.setIdleProposalSettings(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["idleProposalSettings"] });
      toast.success(t("settings.idleProposal.saved"));
    },
    onError: (e) => toast.error(`${t("settings.idleProposal.saveFailed")}: ${describeError(e)}`),
  });

  const enabled = settingsQuery.data?.enabled ?? false;
  const timeoutMinutes = settingsQuery.data?.timeoutMinutes ?? 15;

  return (
    <section
      aria-label={t("settings.idleProposal.heading")}
      className="sketch-card bg-surface-raised p-3"
    >
      <h4 className="mb-2 text-xs font-semibold text-ink">{t("settings.idleProposal.heading")}</h4>
      <p className="mb-2 text-[10px] text-ink-muted">{t("settings.idleProposal.description")}</p>
      <label className="mb-2 flex items-center gap-2 text-xs text-ink-muted">
        <input
          type="checkbox"
          checked={enabled}
          aria-label={t("settings.idleProposal.enabled")}
          onChange={(e) => {
            if (!settingsQuery.data) return;
            setMut.mutate({ ...settingsQuery.data, enabled: e.target.checked });
          }}
          className="accent-[var(--nuomi-accent)]"
        />
        {t("settings.idleProposal.enabled")}
      </label>
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("settings.idleProposal.timeoutMinutes")}
        <input
          type="number"
          min={1}
          value={timeoutMinutes}
          aria-label={t("settings.idleProposal.timeoutMinutes")}
          onChange={(e) => {
            if (!settingsQuery.data) return;
            const parsed = parseInt(e.target.value, 10);
            if (!Number.isFinite(parsed) || parsed < 1) return;
            setMut.mutate({ ...settingsQuery.data, timeoutMinutes: parsed });
          }}
          className="w-24 rounded border border-ink-muted/40 bg-surface px-1.5 py-0.5 text-xs"
        />
      </label>
    </section>
  );
}
