import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { CliFlavorDto } from "../../lib/ipc/bindings.gen";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { CliAgentForm } from "./CliAgentForm";
import type { BindingMode } from "./RoleForm";
import { RoleBindingPanel } from "./RoleBindingPanel";

const FLAVOR_LABEL_KEYS: Record<CliFlavorDto, string> = {
  claude_code: "settings.cliAgents.flavorClaudeCode",
  codex: "settings.cliAgents.flavorCodex",
  plain: "settings.cliAgents.flavorPlain",
};

/** Settings section listing CLI agent profiles with check/delete actions. */
export function CliAgentsSection(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const query = useQuery({ queryKey: ["agentProfiles"], queryFn: ipc.listAgentProfiles });
  /** profileId → version line reported by its latest successful check */
  const [versions, setVersions] = useState<Record<string, string>>({});
  /** profileId awaiting a second click on Delete (two-step confirm, keyboard friendly) */
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const [bindingPreset, setBindingPreset] = useState<
    { mode: BindingMode; providerId?: string; agentProfileId?: string } | null
  >(null);

  const checkMut = useMutation({
    mutationFn: (profileId: string) => ipc.checkCliAgent(profileId),
    onSuccess: (result, profileId) => {
      if (result.ok && result.versionLine !== null) {
        const line = result.versionLine;
        setVersions((prev) => ({ ...prev, [profileId]: line }));
      } else if (!result.ok && result.error !== null) {
        toast.error(`${t("settings.cliAgents.checkFailed")}: ${result.error}`);
      }
    },
    onError: (e) => toast.error(`${t("settings.cliAgents.checkFailed")}: ${describeError(e)}`),
  });

  const deleteMut = useMutation({
    mutationFn: (profileId: string) => ipc.deleteAgentProfile(profileId),
    onSuccess: () => {
      setConfirmingId(null);
      void qc.invalidateQueries({ queryKey: ["agentProfiles"] });
      toast.success(t("settings.cliAgents.deleted"));
    },
    onError: (e) => toast.error(`${t("settings.cliAgents.deleteFailed")}: ${describeError(e)}`),
  });

  return (
    <section aria-label={t("settings.cliAgents.heading")} className="mb-3">
      <h3 className="mb-2 text-sm font-semibold text-ink">{t("settings.cliAgents.heading")}</h3>
      <CliAgentForm />
      <AsyncBoundary
        isLoading={query.isLoading}
        error={query.error}
        isEmpty={(query.data?.length ?? 0) === 0}
        emptyLabel={t("settings.cliAgents.empty")}
        onRetry={() => void query.refetch()}
      >
        <ul className="flex flex-col gap-2">
          {(query.data ?? []).map((profile) => (
            <li key={profile.id} className="rounded border border-ink-muted/40 bg-surface-raised p-3 text-xs">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-sm font-medium text-ink">{profile.name}</span>
                <span className="rounded bg-ink-accent/20 px-1.5 py-0.5 text-[10px] text-ink-accent">
                  {t(FLAVOR_LABEL_KEYS[profile.flavor])}
                </span>
                <span className={profile.enabled ? "text-state-ok" : "text-ink-muted"}>
                  {profile.enabled ? t("common.enabled") : t("common.disabled")}
                </span>
              </div>
              <p className="mt-0.5 font-mono text-ink-muted">
                {[profile.command, ...profile.args].join(" ")}
              </p>
              {profile.workingDir !== null && (
                <p className="mt-0.5 font-mono text-[10px] text-ink-muted">{profile.workingDir}</p>
              )}
              {versions[profile.id] !== undefined && (
                <p className="mt-0.5 text-state-ok">
                  {t("settings.cliAgents.checkOk")} · {versions[profile.id]}
                </p>
              )}
              <div className="mt-1.5 flex gap-2">
                <button
                  type="button"
                  onClick={() => setBindingPreset({ mode: "cli", agentProfileId: profile.id })}
                  className="rounded border border-ink-accent px-2 py-0.5 text-xs text-ink-accent hover:bg-ink-accent/10 focus-visible:ring-2 focus-visible:ring-ink-accent"
                >
                  {t("settings.roles.bindToRole")}
                </button>
                <button
                  type="button"
                  onClick={() => checkMut.mutate(profile.id)}
                  disabled={checkMut.isPending}
                  aria-label={`${t("settings.cliAgents.check")} ${profile.name}`}
                  className="rounded border border-ink-muted px-2 py-0.5 text-xs text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
                >
                  {checkMut.isPending ? t("settings.cliAgents.checking") : t("settings.cliAgents.check")}
                </button>
                {confirmingId === profile.id ? (
                  <button
                    type="button"
                    onClick={() => deleteMut.mutate(profile.id)}
                    disabled={deleteMut.isPending}
                    aria-label={`${t("settings.cliAgents.deleteConfirm")} ${profile.name}`}
                    className="rounded border border-state-danger px-2 py-0.5 text-xs text-state-danger hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
                  >
                    {t("settings.cliAgents.deleteConfirm")}
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={() => setConfirmingId(profile.id)}
                    aria-label={`${t("common.delete")} ${profile.name}`}
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
      {bindingPreset && (
        <RoleBindingPanel presetBinding={bindingPreset} onClose={() => setBindingPreset(null)} />
      )}
    </section>
  );
}
