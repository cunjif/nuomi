import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { CapabilityDto, RoleDto } from "../../lib/ipc/bindings.gen";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { readAgentProfileId, isRoleReady } from "../../lib/conversation/roleReady";
import { RoleBindingPanel } from "./RoleBindingPanel";
import { RoleForm } from "./RoleForm";
import { RoleQuickBinding } from "./RoleQuickBinding";

interface BindingBadgeProps {
  role: RoleDto;
  providerNames: Map<string, string>;
  profileNames: Map<string, string>;
  profileModelIds: Map<string, string | null>;
  providerModelIds: Map<string, string[]>;
}

/** Shows what the role binds to: Provider/ModelId or CLI Agent/ModelId or 默认. */
function BindingBadge({
  role,
  providerNames,
  profileNames,
  profileModelIds,
  providerModelIds,
}: BindingBadgeProps): ReactNode {
  const { t } = useTranslation();
  let label = t("settings.roles.defaultBinding");
  const profileId = readAgentProfileId(role.params);
  if (profileId !== null) {
    const agentName = profileNames.get(profileId) ?? profileId;
    const modelId = profileModelIds.get(profileId) ?? null;
    label = modelId !== null
      ? t("settings.roles.badgeCliAgentModel", { name: agentName, model: modelId })
      : t("settings.roles.badgeCliAgent", { name: agentName });
  } else if (role.providerId !== null) {
    const providerName = providerNames.get(role.providerId) ?? role.providerId;
    const modelIds = providerModelIds.get(role.providerId) ?? [];
    label = modelIds.length > 0
      ? t("settings.roles.badgeProviderModel", { name: providerName, model: modelIds[0] })
      : t("settings.roles.badgeProvider", { name: providerName });
  }
  return (
    <span className="rounded bg-ink-accent/20 px-1.5 py-0.5 text-[10px] text-ink-accent">
      {label}
    </span>
  );
}

/** Small capability badges (Re/I/Vo/Vi) for a role's required capabilities. */
const CAP_BADGES: ReadonlyArray<{ key: CapabilityDto; short: string; cls: string }> = [
  { key: "reasoning", short: "Re", cls: "border-cap-re text-cap-re" },
  { key: "image", short: "I", cls: "border-cap-i text-cap-i" },
  { key: "voice", short: "Vo", cls: "border-cap-vo text-cap-vo" },
  { key: "video", short: "Vi", cls: "border-cap-vi text-cap-vi" },
];

function CapabilityBadges({ role }: { role: RoleDto }): ReactNode {
  if (role.requiredCapabilities.length === 0) return null;
  return (
    <span className="flex items-center gap-1">
      {CAP_BADGES.filter((c) => role.requiredCapabilities.includes(c.key)).map((c) => (
        <span
          key={c.key}
          title={c.key}
          className={`rounded border border-dashed px-1 py-0.5 font-scribble text-[10px] leading-none ${c.cls}`}
        >
          {c.short}
        </span>
      ))}
    </span>
  );
}

/** One role row with binding badge, capabilities, edit and delete. */
function RoleRow({
  role,
  providerNames,
  profileNames,
  profileModelIds,
  providerModelIds,
  confirmingId,
  setConfirmingId,
  deleteMut,
  onOpenBinding,
}: {
  role: RoleDto;
  providerNames: Map<string, string>;
  profileNames: Map<string, string>;
  profileModelIds: Map<string, string | null>;
  providerModelIds: Map<string, string[]>;
  confirmingId: string | null;
  setConfirmingId: (id: string | null) => void;
  deleteMut: { isPending: boolean; mutate: (id: string) => void };
  onOpenBinding: (role: RoleDto) => void;
}): ReactNode {
  const { t } = useTranslation();
  const protectedRole = role.builtin;
  return (
    <li className="sketch-card bg-surface-raised p-3 text-xs">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm font-medium text-ink">{role.name}</span>
        <BindingBadge
          role={role}
          providerNames={providerNames}
          profileNames={profileNames}
          profileModelIds={profileModelIds}
          providerModelIds={providerModelIds}
        />
        <CapabilityBadges role={role} />
        {role.generated && (
          <span className="rounded border border-dashed border-cap-vi px-1.5 py-0.5 font-scribble text-[10px] leading-none text-cap-vi">
            {t("settings.roles.badgeGenerated")}
          </span>
        )}
        {protectedRole && (
          <span className="rounded border border-dashed border-ink-muted px-1.5 py-0.5 font-scribble text-[10px] leading-none text-ink-muted">
            {t("settings.roles.badgeBuiltin")}
          </span>
        )}
      </div>
      {role.systemPromptOverride !== null && (
        <p className="mt-1 line-clamp-2 whitespace-pre-wrap font-mono text-[10px] text-ink-muted">
          {role.systemPromptOverride}
        </p>
      )}
      <div className="mt-1.5 flex gap-2">
        <button
          type="button"
          onClick={() => onOpenBinding(role)}
          aria-label={`${t("settings.roles.editBinding")} ${role.name}`}
          className="rounded border border-ink-accent px-2 py-0.5 text-xs text-ink-accent hover:bg-ink-accent/10 focus-visible:ring-2 focus-visible:ring-ink-accent"
        >
          {t("settings.roles.editBinding")}
        </button>
        {protectedRole ? null : confirmingId === role.id ? (
          <button
            type="button"
            onClick={() => deleteMut.mutate(role.id)}
            disabled={deleteMut.isPending}
            aria-label={`${t("settings.roles.deleteConfirm")} ${role.name}`}
            className="rounded border border-state-danger px-2 py-0.5 text-xs text-state-danger hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
          >
            {t("settings.roles.deleteConfirm")}
          </button>
        ) : (
          <button
            type="button"
            onClick={() => setConfirmingId(role.id)}
            aria-label={`${t("common.delete")} ${role.name}`}
            className="rounded border border-ink-muted px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("common.delete")}
          </button>
        )}
      </div>
    </li>
  );
}

/** Settings section: Role form + quick binding + flat role list. */
export function RolesSection(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const query = useQuery({ queryKey: ["roles"], queryFn: ipc.listRoles });
  const providersQuery = useQuery({ queryKey: ["providers"], queryFn: ipc.listProviders });
  const profilesQuery = useQuery({ queryKey: ["agentProfiles"], queryFn: ipc.listAgentProfiles });
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const [bindingPanelRole, setBindingPanelRole] = useState<RoleDto | null>(null);

  const deleteMut = useMutation({
    mutationFn: (roleId: string) => ipc.deleteRole(roleId),
    onSuccess: () => {
      setConfirmingId(null);
      void qc.invalidateQueries({ queryKey: ["roles"] });
      toast.success(t("settings.roles.deleted"));
    },
    onError: (e) => toast.error(`${t("settings.roles.deleteFailed")}: ${describeError(e)}`),
  });

  const providerNames = new Map((providersQuery.data ?? []).map((p) => [p.id, p.name]));
  const profileNames = new Map((profilesQuery.data ?? []).map((p) => [p.id, p.name]));
  const profileModelIds = new Map((profilesQuery.data ?? []).map((p) => [p.id, p.modelId]));
  const providerModelIds = new Map(
    (providersQuery.data ?? []).map((p) => [p.id, (p.settings.models ?? []).map((m) => m.id)]),
  );
  const roles = query.data ?? [];
  const roleAgents = roles.filter(isRoleReady);

  return (
    <section aria-label={t("settings.roles.heading")} className="mb-3">
      <h3 className="mb-2 text-sm font-semibold text-ink">{t("settings.roles.heading")}</h3>
      <RoleForm />
      <RoleQuickBinding />
      <AsyncBoundary
        isLoading={query.isLoading}
        error={query.error}
        isEmpty={roleAgents.length === 0}
        emptyLabel={t("settings.roles.emptyAgents")}
        onRetry={() => void query.refetch()}
      >
        <ul className="flex flex-col gap-2">
          {roleAgents.map((role) => (
            <RoleRow
              key={role.id}
              role={role}
              providerNames={providerNames}
              profileNames={profileNames}
              profileModelIds={profileModelIds}
              providerModelIds={providerModelIds}
              confirmingId={confirmingId}
              setConfirmingId={setConfirmingId}
              deleteMut={deleteMut}
              onOpenBinding={(r) => setBindingPanelRole(r)}
            />
          ))}
        </ul>
      </AsyncBoundary>
      {bindingPanelRole && (
        <RoleBindingPanel initialRole={bindingPanelRole} onClose={() => setBindingPanelRole(null)} />
      )}
    </section>
  );
}
