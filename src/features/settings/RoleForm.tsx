import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { JsonValue, RoleInput } from "../../lib/ipc/bindings.gen";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

/** Binding mode of the role form: unbound ("默认"), provider or CLI agent profile. */
export type BindingMode = "none" | "provider" | "cli";

/** Reads the `agent_profile_id` convention key out of a role's params JSON. */
export function readAgentProfileId(params: JsonValue): string | null {
  if (params !== null && typeof params === "object" && !Array.isArray(params)) {
    const value = params["agent_profile_id"];
    if (typeof value === "string") return value;
  }
  return null;
}

/** Add form for roles (`name` is the backend idempotency key). */
export function RoleForm(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [bindingMode, setBindingMode] = useState<BindingMode>("none");
  const [providerId, setProviderId] = useState("");
  const [agentProfileId, setAgentProfileId] = useState("");
  const [systemPromptOverride, setSystemPromptOverride] = useState("");

  const providersQuery = useQuery({ queryKey: ["providers"], queryFn: ipc.listProviders });
  const profilesQuery = useQuery({ queryKey: ["agentProfiles"], queryFn: ipc.listAgentProfiles });

  const saveMut = useMutation({
    mutationFn: (input: RoleInput) => ipc.upsertRole(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["roles"] });
      toast.success(t("settings.roles.saved"));
      setName("");
      setBindingMode("none");
      setProviderId("");
      setAgentProfileId("");
      setSystemPromptOverride("");
    },
    onError: (e) => {
      // Inputs stay as-is so the user can fix and resubmit.
      toast.error(`${t("settings.roles.saveFailed")}: ${describeError(e)}`);
    },
  });

  const field =
    "rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent";

  return (
    <form
      aria-label={t("settings.roles.heading")}
      className="mb-3 rounded border border-ink-muted/40 bg-surface-raised p-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (!name.trim() || saveMut.isPending) return;
        const boundProvider = bindingMode === "provider" ? providerId : null;
        const boundAgent = bindingMode === "cli" ? agentProfileId : null;
        if (bindingMode === "provider" && boundProvider === "") return;
        if (bindingMode === "cli" && boundAgent === "") return;
        saveMut.mutate({
          name: name.trim(),
          // CLI-agent binding wins over provider (SPEC team-shell-m1 D2b).
          providerId: boundProvider,
          systemPromptOverride:
            systemPromptOverride.trim().length > 0 ? systemPromptOverride.trim() : null,
          toolAllowlist: [],
          temperature: null,
          maxTokens: null,
          params:
            boundAgent !== null
              ? { agent_profile_id: boundAgent }
              : {},
        });
      }}
    >
      <p className="mb-2 text-xs text-ink-muted">{t("settings.roles.idempotentHint")}</p>
      <div className="flex flex-wrap items-end gap-2">
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.roles.name")}
          <input value={name} onChange={(e) => setName(e.target.value)} required className={`${field} w-40`} />
        </label>
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.roles.bindingMode")}
          <select
            value={bindingMode}
            onChange={(e) => {
              const mode = e.target.value;
              if (mode === "provider" || mode === "cli" || mode === "none") setBindingMode(mode);
            }}
            className={field}
          >
            <option value="none">{t("settings.roles.bindingNone")}</option>
            <option value="provider">{t("settings.roles.bindingProvider")}</option>
            <option value="cli">{t("settings.roles.bindingCliAgent")}</option>
          </select>
        </label>
        {bindingMode === "provider" && (
          <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
            {t("settings.roles.providerTarget")}
            <select
              value={providerId}
              onChange={(e) => setProviderId(e.target.value)}
              required
              className={`${field} w-44`}
            >
              <option value="" disabled>
                {t("settings.roles.providerTarget")}
              </option>
              {(providersQuery.data ?? []).map((provider) => (
                <option key={provider.id} value={provider.id}>
                  {provider.name}
                </option>
              ))}
            </select>
          </label>
        )}
        {bindingMode === "cli" && (
          <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
            {t("settings.roles.agentProfileTarget")}
            <select
              value={agentProfileId}
              onChange={(e) => setAgentProfileId(e.target.value)}
              required
              className={`${field} w-44`}
            >
              <option value="" disabled>
                {t("settings.roles.agentProfileTarget")}
              </option>
              {(profilesQuery.data ?? []).map((profile) => (
                <option key={profile.id} value={profile.id}>
                  {profile.name}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
      <label className="mt-2 flex min-w-48 flex-1 flex-col gap-0.5 text-xs text-ink-muted">
        {t("settings.roles.systemPromptOverride")}
        <textarea
          value={systemPromptOverride}
          onChange={(e) => setSystemPromptOverride(e.target.value)}
          rows={3}
          spellCheck={false}
          className={`${field} font-mono`}
        />
      </label>
      <button
        type="submit"
        disabled={saveMut.isPending}
        className="mt-2 rounded bg-ink-accent px-3 py-1.5 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      >
        {saveMut.isPending ? t("settings.roles.saving") : t("settings.roles.save")}
      </button>
    </form>
  );
}
