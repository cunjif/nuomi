import type { ReactNode } from "react";
import { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { CapabilityDto, RoleInput } from "../../lib/ipc/bindings.gen";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { fieldClass as field } from "../../components/ui/Field";
import { CAPABILITY_KEYS } from "./RoleForm";
import { ToolTagInput } from "./ToolTagInput";
import { buildBindingOptions, decodeBindingValue } from "./bindingOptions";

/** Quick binding form: create a Role Agent instance from a Role template + model binding. */
export function RoleQuickBinding(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [sourceRoleId, setSourceRoleId] = useState("");
  const [bindingValue, setBindingValue] = useState("");
  const [systemPromptOverride, setSystemPromptOverride] = useState("");
  const [requiredCapabilities, setRequiredCapabilities] = useState<CapabilityDto[]>([]);
  const [toolAllowlist, setToolAllowlist] = useState<string[]>([]);
  const hasInherited = useRef(false);

  const rolesQuery = useQuery({ queryKey: ["roles"], queryFn: ipc.listRoles });
  const providersQuery = useQuery({ queryKey: ["providers"], queryFn: ipc.listProviders });
  const profilesQuery = useQuery({ queryKey: ["agentProfiles"], queryFn: ipc.listAgentProfiles });

  const roles = rolesQuery.data ?? [];
  const bindingOptions = useMemo(
    () => buildBindingOptions(providersQuery.data ?? [], profilesQuery.data ?? []),
    [providersQuery.data, profilesQuery.data],
  );
  const providerOpts = bindingOptions.filter((o) => o.group === "provider");
  const cliOpts = bindingOptions.filter((o) => o.group === "cli");

  const toolCandidates = useMemo(() => {
    const tools = new Set<string>();
    for (const p of providersQuery.data ?? []) {
      for (const m of p.settings.models ?? []) {
        for (const cap of m.capabilities) tools.add(cap);
      }
    }
    return [...tools].sort();
  }, [providersQuery.data]);

  const saveMut = useMutation({
    mutationFn: (input: RoleInput) => ipc.upsertRole(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["roles"] });
      toast.success(t("settings.roles.quickBindingSaved"));
      setName("");
      setSourceRoleId("");
      setBindingValue("");
      setSystemPromptOverride("");
      setRequiredCapabilities([]);
      setToolAllowlist([]);
      hasInherited.current = false;
    },
    onError: (e) => {
      toast.error(`${t("settings.roles.quickBindingFailed")}: ${describeError(e)}`);
    },
  });

  const onSourceRoleChange = (roleId: string) => {
    setSourceRoleId(roleId);
    if (hasInherited.current) return;
    const role = roles.find((r) => r.id === roleId);
    if (role) {
      setSystemPromptOverride(role.systemPromptOverride ?? "");
      setRequiredCapabilities([...role.requiredCapabilities]);
      hasInherited.current = true;
    }
  };

  return (
    <form
      aria-label={t("settings.roles.quickBindingHeading")}
      className="mb-3 rounded border border-ink-muted/40 bg-surface p-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (!name.trim() || !bindingValue || saveMut.isPending) return;
        const decoded = decodeBindingValue(bindingValue);
        if (decoded === null) return;
        const base: RoleInput = {
          name: name.trim(),
          providerId: null,
          providerIds: [],
          systemPromptOverride:
            systemPromptOverride.trim().length > 0 ? systemPromptOverride.trim() : null,
          toolAllowlist,
          requiredCapabilities,
          temperature: null,
          maxTokens: null,
          params: {},
        };
        if (decoded.kind === "provider" && decoded.providerId) {
          base.providerId = decoded.providerId;
          base.providerIds = [decoded.providerId];
        } else if (decoded.kind === "cli" && decoded.agentProfileId) {
          base.params = { agent_profile_id: decoded.agentProfileId };
        }
        saveMut.mutate(base);
      }}
    >
      <p className="mb-2 text-xs text-ink-muted">{t("settings.roles.quickBindingHint")}</p>
      <div className="flex flex-wrap items-end gap-2">
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.roles.quickBindingName")}
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            required
            placeholder="coder-agent"
            className={`${field} bg-surface-raised text-sm w-40`}
          />
        </label>
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.roles.quickBindingSourceRole")}
          <select
            value={sourceRoleId}
            onChange={(e) => onSourceRoleChange(e.target.value)}
            className={`${field} bg-surface-raised text-sm w-40`}
          >
            <option value="">{t("settings.roles.quickBindingNoSource")}</option>
            {roles.map((r) => (
              <option key={r.id} value={r.id}>
                {r.name}
              </option>
            ))}
          </select>
        </label>
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.roles.quickBindingTarget")}
          <select
            value={bindingValue}
            onChange={(e) => setBindingValue(e.target.value)}
            required
            className={`${field} bg-surface-raised text-sm w-56`}
          >
            <option value="" disabled>
              {t("settings.roles.quickBindingSelectTarget")}
            </option>
            {providerOpts.length > 0 && (
              <optgroup label={t("settings.roles.quickBindingProviderGroup")}>
                {providerOpts.map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label}
                  </option>
                ))}
              </optgroup>
            )}
            {cliOpts.length > 0 && (
              <optgroup label={t("settings.roles.quickBindingCliGroup")}>
                {cliOpts.map((o) => (
                  <option key={o.value} value={o.value}>
                    {o.label}
                  </option>
                ))}
              </optgroup>
            )}
          </select>
        </label>
      </div>
      <div className="mt-2 flex flex-wrap items-center gap-1.5">
        <span className="text-xs text-ink-muted">{t("settings.roles.requiredCapabilities")}</span>
        {CAPABILITY_KEYS.map((cap) => {
          const active = requiredCapabilities.includes(cap.key);
          return (
            <label
              key={cap.key}
              className={`flex cursor-pointer items-center gap-1 rounded-full border px-2 py-0.5 text-xs focus-within:ring-2 focus-within:ring-ink-accent ${
                active
                  ? "border-ink-accent bg-ink-accent/20 text-ink"
                  : "border-ink-muted/40 text-ink-muted hover:bg-surface-overlay"
              }`}
            >
              <input
                type="checkbox"
                className="sr-only"
                checked={active}
                onChange={() =>
                  setRequiredCapabilities((prev) =>
                    prev.includes(cap.key)
                      ? prev.filter((c) => c !== cap.key)
                      : [...prev, cap.key],
                  )
                }
              />
              {t(cap.labelKey)}
            </label>
          );
        })}
      </div>
      <label className="mt-2 flex min-w-48 flex-1 flex-col gap-0.5 text-xs text-ink-muted">
        {t("settings.roles.systemPromptOverride")}
        <textarea
          value={systemPromptOverride}
          onChange={(e) => setSystemPromptOverride(e.target.value)}
          rows={2}
          spellCheck={false}
          className={`${field} bg-surface-raised text-sm font-mono`}
        />
      </label>
      <label className="mt-2 flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("settings.roles.toolAllowlist")}
        <ToolTagInput
          value={toolAllowlist}
          onChange={setToolAllowlist}
          candidates={toolCandidates}
          placeholder={t("settings.roles.toolAllowlistPlaceholder")}
        />
      </label>
      <button
        type="submit"
        disabled={saveMut.isPending}
        className="mt-2 pixel-fill-accent px-3 py-1.5 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      >
        {saveMut.isPending ? t("settings.roles.saving") : t("settings.roles.quickBindingSave")}
      </button>
    </form>
  );
}
