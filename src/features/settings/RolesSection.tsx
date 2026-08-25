import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { RoleDto } from "../../lib/ipc/bindings.gen";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { readAgentProfileId, RoleForm } from "./RoleForm";

interface BindingBadgeProps {
  role: RoleDto;
  providerNames: Map<string, string>;
  profileNames: Map<string, string>;
}

/** Shows what the role binds to: a provider name, a CLI agent name or 默认. */
function BindingBadge({ role, providerNames, profileNames }: BindingBadgeProps): ReactNode {
  const { t } = useTranslation();
  let label = t("settings.roles.defaultBinding");
  const profileId = readAgentProfileId(role.params);
  if (profileId !== null) {
    label = t("settings.roles.badgeCliAgent", {
      name: profileNames.get(profileId) ?? profileId,
    });
  } else if (role.providerId !== null) {
    label = t("settings.roles.badgeProvider", {
      name: providerNames.get(role.providerId) ?? role.providerId,
    });
  }
  return (
    <span className="rounded bg-ink-accent/20 px-1.5 py-0.5 text-[10px] text-ink-accent">
      {label}
    </span>
  );
}

/** Settings section listing roles with binding badges and delete actions. */
export function RolesSection(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const query = useQuery({ queryKey: ["roles"], queryFn: ipc.listRoles });
  const providersQuery = useQuery({ queryKey: ["providers"], queryFn: ipc.listProviders });
  const profilesQuery = useQuery({ queryKey: ["agentProfiles"], queryFn: ipc.listAgentProfiles });
  /** roleId awaiting a second click on Delete (two-step confirm, keyboard friendly) */
  const [confirmingId, setConfirmingId] = useState<string | null>(null);

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

  return (
    <section aria-label={t("settings.roles.heading")} className="mb-3">
      <h3 className="mb-2 text-sm font-semibold text-ink">{t("settings.roles.heading")}</h3>
      <RoleForm />
      <AsyncBoundary
        isLoading={query.isLoading}
        error={query.error}
        isEmpty={(query.data?.length ?? 0) === 0}
        emptyLabel={t("settings.roles.empty")}
        onRetry={() => void query.refetch()}
      >
        <ul className="flex flex-col gap-2">
          {(query.data ?? []).map((role) => (
            <li key={role.id} className="rounded border border-ink-muted/40 bg-surface-raised p-3 text-xs">
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-sm font-medium text-ink">{role.name}</span>
                <BindingBadge
                  role={role}
                  providerNames={providerNames}
                  profileNames={profileNames}
                />
              </div>
              {role.systemPromptOverride !== null && (
                <p className="mt-1 whitespace-pre-wrap font-mono text-[10px] text-ink-muted">
                  {role.systemPromptOverride}
                </p>
              )}
              <div className="mt-1.5 flex gap-2">
                {confirmingId === role.id ? (
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
          ))}
        </ul>
      </AsyncBoundary>
    </section>
  );
}
