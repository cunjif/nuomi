import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { CapabilityDto, RoleDto, RoleInput } from "../../lib/ipc/bindings.gen";
import { readAgentProfileId } from "../../lib/conversation/roleReady";
import { CAPABILITY_KEYS, type BindingMode } from "./RoleForm";
import { ToolTagInput } from "./ToolTagInput";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { fieldClass as field } from "../../components/ui/Field";

export interface RoleBindingPanelProps {
  /** Edit mode: prefill from an existing role. */
  initialRole?: RoleDto;
  /** Reverse-guidance mode: prefill binding target from Provider/CLI Agent detail page. */
  presetBinding?: { mode: BindingMode; providerId?: string; agentProfileId?: string };
  onClose: () => void;
}

function inferBindingMode(role: RoleDto): BindingMode {
  if (readAgentProfileId(role.params) !== null) return "cli";
  if (role.providerId !== null) return "provider";
  return "none";
}

function parseOptionalFloat(s: string): number | null {
  const trimmed = s.trim();
  if (trimmed === "") return null;
  const n = Number.parseFloat(trimmed);
  return Number.isNaN(n) ? null : n;
}

function parseOptionalInt(s: string): number | null {
  const trimmed = s.trim();
  if (trimmed === "") return null;
  const n = Number.parseInt(trimmed, 10);
  return Number.isNaN(n) ? null : n;
}

/** Full-field binding panel for creating or editing a Role's provider/CLI binding + overlay params. */
export function RoleBindingPanel({ initialRole, presetBinding, onClose }: RoleBindingPanelProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const editing = initialRole !== undefined;

  const [name, setName] = useState(initialRole?.name ?? "");
  const [bindingMode, setBindingMode] = useState<BindingMode>(
    presetBinding?.mode ?? (initialRole ? inferBindingMode(initialRole) : "none"),
  );
  const [providerId, setProviderId] = useState(
    presetBinding?.providerId ?? initialRole?.providerId ?? "",
  );
  const [agentProfileId, setAgentProfileId] = useState(
    presetBinding?.agentProfileId ?? (initialRole ? (readAgentProfileId(initialRole.params) ?? "") : ""),
  );
  const [systemPromptOverride, setSystemPromptOverride] = useState(initialRole?.systemPromptOverride ?? "");
  const [temperature, setTemperature] = useState(
    initialRole?.temperature != null ? String(initialRole.temperature) : "",
  );
  const [maxTokens, setMaxTokens] = useState(
    initialRole?.maxTokens != null ? String(initialRole.maxTokens) : "",
  );
  const [toolAllowlist, setToolAllowlist] = useState<string[]>(initialRole?.toolAllowlist ?? []);
  const [requiredCapabilities, setRequiredCapabilities] = useState<CapabilityDto[]>(
    initialRole?.requiredCapabilities ?? [],
  );

  const providersQuery = useQuery({ queryKey: ["providers"], queryFn: ipc.listProviders });
  const profilesQuery = useQuery({ queryKey: ["agentProfiles"], queryFn: ipc.listAgentProfiles });

  const saveMut = useMutation({
    mutationFn: (input: RoleInput) => ipc.upsertRole(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["roles"] });
      toast.success(t("settings.roles.saved"));
      onClose();
    },
    onError: (e) => toast.error(`${t("settings.roles.saveFailed")}: ${describeError(e)}`),
  });

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={t("settings.roles.heading")}
    >
      <div className="w-full max-w-lg rounded border border-ink-muted/40 bg-surface-raised p-4">
        <h4 className="mb-2 text-sm font-semibold text-ink">
          {editing ? t("settings.roles.editBinding") : t("settings.roles.bindToRole")}
        </h4>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (!name.trim() || saveMut.isPending) return;
            const boundProvider = bindingMode === "provider" ? providerId : null;
            const boundAgent = bindingMode === "cli" ? agentProfileId : null;
            if (bindingMode === "provider" && boundProvider === "") return;
            if (bindingMode === "cli" && boundAgent === "") return;
            saveMut.mutate({
              name: name.trim(),
              providerId: boundProvider,
              providerIds: boundProvider !== null ? [boundProvider] : [],
              systemPromptOverride:
                systemPromptOverride.trim().length > 0 ? systemPromptOverride.trim() : null,
              toolAllowlist,
              requiredCapabilities,
              temperature: parseOptionalFloat(temperature),
              maxTokens: parseOptionalInt(maxTokens),
              params: boundAgent !== null ? { agent_profile_id: boundAgent } : {},
            });
          }}
        >
          <div className="flex flex-wrap items-end gap-2">
            <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
              {t("settings.roles.name")}
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                disabled={editing}
                required
                className={`${field} bg-surface text-sm w-40`}
              />
            </label>
            <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
              {t("settings.roles.bindingMode")}
              <select
                value={bindingMode}
                onChange={(e) => {
                  const mode = e.target.value;
                  if (mode === "provider" || mode === "cli" || mode === "none") setBindingMode(mode);
                }}
                className={`${field} bg-surface text-sm`}
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
                  className={`${field} bg-surface text-sm w-44`}
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
                  className={`${field} bg-surface text-sm w-44`}
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
          <div className="mt-2 flex flex-wrap gap-2">
            <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
              {t("settings.roles.temperature")}
              <input
                type="number"
                value={temperature}
                onChange={(e) => setTemperature(e.target.value)}
                step="0.1"
                className={`${field} bg-surface text-sm w-24`}
              />
            </label>
            <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
              {t("settings.roles.maxTokens")}
              <input
                type="number"
                value={maxTokens}
                onChange={(e) => setMaxTokens(e.target.value)}
                step="1"
                className={`${field} bg-surface text-sm w-28`}
              />
            </label>
            <label className="flex flex-1 flex-col gap-0.5 text-xs text-ink-muted">
              {t("settings.roles.toolAllowlist")}
              <ToolTagInput
                value={toolAllowlist}
                onChange={setToolAllowlist}
                candidates={[]}
                placeholder={t("settings.roles.toolAllowlistHint")}
              />
            </label>
          </div>
          <label className="mt-2 flex min-w-48 flex-1 flex-col gap-0.5 text-xs text-ink-muted">
            {t("settings.roles.systemPromptOverride")}
            <textarea
              value={systemPromptOverride}
              onChange={(e) => setSystemPromptOverride(e.target.value)}
              rows={3}
              spellCheck={false}
              className={`${field} bg-surface text-sm font-mono`}
            />
          </label>
          <div className="mt-3 flex justify-end gap-2">
            <button
              type="button"
              onClick={onClose}
              className="rounded border border-ink-muted px-3 py-1 text-sm text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
            >
              {t("common.cancel")}
            </button>
            <button
              type="submit"
              disabled={saveMut.isPending}
              className="pixel-fill-accent px-3 py-1 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
            >
              {saveMut.isPending ? t("settings.roles.saving") : t("settings.roles.save")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
