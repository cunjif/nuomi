import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import type { RoleDirectorBindingDto, RoleDirectorBindingModeDto } from "../../lib/ipc/bindings.gen";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { fieldClass as field } from "../../components/ui/Field";

const BINDING_QUERY_KEY = ["role-director-binding"] as const;

/**
 * Role Director self-binding panel: lets the director pick its OWN Provider
 * or CLI Agent (the model it uses to orchestrate). This binding is NOT
 * inherited by the roles it generates.
 *
 * Mounted in the top-right of the RoleDirectorDialog. Reports the current
 * binding (or null) back to the parent via `onBindingChange` so the generate
 * button can be gated on "director has a model to run on".
 */
export function RoleDirectorBindingPanel({
  onBindingChange,
}: {
  onBindingChange: (binding: RoleDirectorBindingDto | null) => void;
}): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();

  const providersQuery = useQuery({
    queryKey: ["providers"],
    queryFn: () => ipc.listProviders(),
  });
  const agentsQuery = useQuery({
    queryKey: ["agent-profiles"],
    queryFn: () => ipc.listAgentProfiles(),
  });
  const bindingQuery = useQuery({
    queryKey: BINDING_QUERY_KEY,
    queryFn: () => ipc.getRoleDirectorBinding(),
  });

  const [mode, setMode] = useState<RoleDirectorBindingModeDto>("provider");
  const [providerId, setProviderId] = useState<string | null>(null);
  const [agentProfileId, setAgentProfileId] = useState<string | null>(null);
  const [hydrated, setHydrated] = useState(false);

  const persistMut = useMutation({
    mutationFn: (binding: RoleDirectorBindingDto) => ipc.setRoleDirectorBinding(binding),
    onSuccess: () => void qc.invalidateQueries({ queryKey: BINDING_QUERY_KEY }),
    onError: (e) => toast.error(`${t("settings.roles.bindingFailed")}: ${describeError(e)}`),
  });

  // Restore persisted binding on first load.
  useEffect(() => {
    if (hydrated || bindingQuery.isLoading || bindingQuery.data === undefined) return;
    const stored = bindingQuery.data ?? null;
    if (stored) {
      setMode(stored.bindingMode);
      setProviderId(stored.providerId);
      setAgentProfileId(stored.agentProfileId);
    }
    setHydrated(true);
  }, [hydrated, bindingQuery.isLoading, bindingQuery.data]);

  // Report binding state to parent whenever it changes.
  useEffect(() => {
    if (!hydrated) return;
    const targetId = mode === "provider" ? providerId : agentProfileId;
    if (!targetId) {
      onBindingChange(null);
      return;
    }
    onBindingChange({
      bindingMode: mode,
      providerId: mode === "provider" ? providerId : null,
      agentProfileId: mode === "cli" ? agentProfileId : null,
    });
  }, [hydrated, mode, providerId, agentProfileId, onBindingChange]);

  const providers = providersQuery.data ?? [];
  const agents = agentsQuery.data ?? [];
  const noModels = providers.length === 0 && agents.length === 0;

  function commit(next: RoleDirectorBindingDto) {
    persistMut.mutate(next);
  }

  function handleModeChange(nextMode: RoleDirectorBindingModeDto) {
    setMode(nextMode);
    const targetId = nextMode === "provider" ? providerId : agentProfileId;
    if (targetId) {
      commit({
        bindingMode: nextMode,
        providerId: nextMode === "provider" ? providerId : null,
        agentProfileId: nextMode === "cli" ? agentProfileId : null,
      });
    }
  }

  function handleProviderChange(id: string) {
    setProviderId(id);
    commit({ bindingMode: "provider", providerId: id, agentProfileId: null });
  }

  function handleAgentChange(id: string) {
    setAgentProfileId(id);
    commit({ bindingMode: "cli", providerId: null, agentProfileId: id });
  }

  return (
    <div className="rounded border border-ink-muted/30 bg-surface p-2 text-xs">
      <div className="mb-1 font-semibold text-ink-muted">
        {t("settings.roles.directorBindingTitle")}
      </div>
      <div className="mb-1 text-[10px] text-ink-muted/70">
        {t("settings.roles.directorBindingHint")}
      </div>
      {noModels ? (
        <p className="text-ink-muted">{t("settings.roles.directorNoModel")}</p>
      ) : (
        <>
          <div className="mb-1 flex gap-2">
            <label className="flex items-center gap-1">
              <input
                type="radio"
                checked={mode === "provider"}
                onChange={() => handleModeChange("provider")}
                disabled={providers.length === 0}
              />
              {t("settings.roles.bindingProvider")}
            </label>
            <label className="flex items-center gap-1">
              <input
                type="radio"
                checked={mode === "cli"}
                onChange={() => handleModeChange("cli")}
                disabled={agents.length === 0}
              />
              {t("settings.roles.bindingCli")}
            </label>
          </div>
          {mode === "provider" && (
            <select
              className={field}
              value={providerId ?? ""}
              onChange={(e) => handleProviderChange(e.target.value)}
              aria-label={t("settings.roles.bindingProvider")}
            >
              <option value="">{t("common.select")}</option>
              {providers.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          )}
          {mode === "cli" && (
            <select
              className={field}
              value={agentProfileId ?? ""}
              onChange={(e) => handleAgentChange(e.target.value)}
              aria-label={t("settings.roles.bindingCli")}
            >
              <option value="">{t("common.select")}</option>
              {agents.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
          )}
        </>
      )}
    </div>
  );
}
