import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { CapabilityDto, RoleDto } from "../../lib/ipc/bindings.gen";
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

/** Small capability badges (Re/I/Vo/Vi) for a role's required capabilities. */
const CAP_BADGES: ReadonlyArray<{ key: CapabilityDto; short: string; cls: string }> = [
  { key: "reasoning", short: "Re", cls: "border-sky-500/60 text-sky-400" },
  { key: "image", short: "I", cls: "border-emerald-500/60 text-emerald-400" },
  { key: "voice", short: "Vo", cls: "border-amber-500/60 text-amber-400" },
  { key: "video", short: "Vi", cls: "border-fuchsia-500/60 text-fuchsia-400" },
];

function CapabilityBadges({ role }: { role: RoleDto }): ReactNode {
  if (role.requiredCapabilities.length === 0) return null;
  return (
    <span className="flex items-center gap-1">
      {CAP_BADGES.filter((c) => role.requiredCapabilities.includes(c.key)).map((c) => (
        <span
          key={c.key}
          title={c.key}
          className={`rounded border px-1 py-0.5 text-[10px] leading-none ${c.cls}`}
        >
          {c.short}
        </span>
      ))}
    </span>
  );
}

/** One grouped role row with binding badge + two-step delete. */
function RoleRow({
  role,
  providerNames,
  profileNames,
  confirmingId,
  setConfirmingId,
  deleteMut,
}: {
  role: RoleDto;
  providerNames: Map<string, string>;
  profileNames: Map<string, string>;
  confirmingId: string | null;
  setConfirmingId: (id: string | null) => void;
  deleteMut: { isPending: boolean; mutate: (id: string) => void };
}): ReactNode {
  const { t } = useTranslation();
  const protectedRole = role.builtin;
  return (
    <li className="rounded border border-ink-muted/40 bg-surface-raised p-3 text-xs">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm font-medium text-ink">{role.name}</span>
        <BindingBadge role={role} providerNames={providerNames} profileNames={profileNames} />
        <CapabilityBadges role={role} />
        {role.generated && (
          <span className="rounded bg-violet-500/20 px-1.5 py-0.5 text-[10px] text-violet-400">
            {t("settings.roles.badgeGenerated")}
          </span>
        )}
        {protectedRole && (
          <span className="rounded bg-ink-muted/20 px-1.5 py-0.5 text-[10px] text-ink-muted">
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

/** Role Director dialog: plain-language description → generated Role. */
function RoleDirectorDialog({ onClose }: { onClose: () => void }): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [description, setDescription] = useState("");

  const generateMut = useMutation({
    mutationFn: (desc: string) => ipc.generateRole(desc),
    onSuccess: (role) => {
      void qc.invalidateQueries({ queryKey: ["roles"] });
      toast.success(t("settings.roles.directorSuccess", { name: role.name }));
      onClose();
    },
    onError: (e) => toast.error(`${t("settings.roles.directorFailed")}: ${describeError(e)}`),
  });

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      role="dialog"
      aria-modal="true"
      aria-label={t("settings.roles.directorTitle")}
    >
      <div className="w-full max-w-md rounded border border-ink-muted/40 bg-surface-raised p-4">
        <h4 className="mb-2 text-sm font-semibold text-ink">
          {t("settings.roles.directorTitle")}
        </h4>
        <p className="mb-2 text-xs text-ink-muted">{t("settings.roles.directorHint")}</p>
        <textarea
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          rows={4}
          autoFocus
          placeholder={t("settings.roles.directorPlaceholder") ?? ""}
          aria-label={t("settings.roles.directorTitle")}
          className="w-full rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink placeholder:text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
        />
        <div className="mt-3 flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-ink-muted px-3 py-1 text-sm text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("common.cancel")}
          </button>
          <button
            type="button"
            disabled={generateMut.isPending || description.trim().length === 0}
            onClick={() => generateMut.mutate(description.trim())}
            className="pixel-fill-accent px-3 py-1 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
          >
            {generateMut.isPending ? t("settings.roles.directorGenerating") : t("settings.roles.directorGenerate")}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Settings section: preset restore, Role Director entry, grouped role list. */
export function RolesSection(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const query = useQuery({ queryKey: ["roles"], queryFn: ipc.listRoles });
  const providersQuery = useQuery({ queryKey: ["providers"], queryFn: ipc.listProviders });
  const profilesQuery = useQuery({ queryKey: ["agentProfiles"], queryFn: ipc.listAgentProfiles });
  /** roleId awaiting a second click on Delete (two-step confirm, keyboard friendly) */
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const [directorOpen, setDirectorOpen] = useState(false);

  const deleteMut = useMutation({
    mutationFn: (roleId: string) => ipc.deleteRole(roleId),
    onSuccess: () => {
      setConfirmingId(null);
      void qc.invalidateQueries({ queryKey: ["roles"] });
      toast.success(t("settings.roles.deleted"));
    },
    onError: (e) => toast.error(`${t("settings.roles.deleteFailed")}: ${describeError(e)}`),
  });

  const seedMut = useMutation({
    mutationFn: () => ipc.seedBuiltinRoles(),
    onSuccess: (report) => {
      void qc.invalidateQueries({ queryKey: ["roles"] });
      toast.success(
        t("settings.roles.presetRestored", {
          inserted: report.inserted,
          updated: report.updated,
        }),
      );
    },
    onError: (e) => toast.error(`${t("settings.roles.presetRestoreFailed")}: ${describeError(e)}`),
  });

  const providerNames = new Map((providersQuery.data ?? []).map((p) => [p.id, p.name]));
  const profileNames = new Map((profilesQuery.data ?? []).map((p) => [p.id, p.name]));
  const roles = query.data ?? [];
  const groups: Array<{ label: string; roles: RoleDto[] }> = [
    { label: t("settings.roles.groupBuiltin"), roles: roles.filter((r) => r.builtin) },
    {
      label: t("settings.roles.groupGenerated"),
      roles: roles.filter((r) => !r.builtin && r.generated),
    },
    {
      label: t("settings.roles.groupCustom"),
      roles: roles.filter((r) => !r.builtin && !r.generated),
    },
  ];

  return (
    <section aria-label={t("settings.roles.heading")} className="mb-3">
      <div className="mb-2 flex items-center justify-between">
        <h3 className="text-sm font-semibold text-ink">{t("settings.roles.heading")}</h3>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => setDirectorOpen(true)}
            className="rounded border border-ink-accent px-2 py-0.5 text-xs text-ink-accent hover:bg-ink-accent/10 focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("settings.roles.directorOpen")}
          </button>
          <button
            type="button"
            onClick={() => seedMut.mutate()}
            disabled={seedMut.isPending}
            className="rounded border border-ink-muted px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
          >
            {seedMut.isPending ? t("settings.roles.restoringPresets") : t("settings.roles.restorePresets")}
          </button>
        </div>
      </div>
      <RoleForm />
      <AsyncBoundary
        isLoading={query.isLoading}
        error={query.error}
        isEmpty={roles.length === 0}
        emptyLabel={t("settings.roles.empty")}
        onRetry={() => void query.refetch()}
      >
        <div className="flex flex-col gap-3">
          {groups.map(
            (group) =>
              group.roles.length > 0 && (
                <div key={group.label}>
                  <p className="mb-1 text-[10px] font-semibold uppercase tracking-wide text-ink-muted">
                    {group.label} ({group.roles.length})
                  </p>
                  <ul className="flex flex-col gap-2">
                    {group.roles.map((role) => (
                      <RoleRow
                        key={role.id}
                        role={role}
                        providerNames={providerNames}
                        profileNames={profileNames}
                        confirmingId={confirmingId}
                        setConfirmingId={setConfirmingId}
                        deleteMut={deleteMut}
                      />
                    ))}
                  </ul>
                </div>
              ),
          )}
        </div>
      </AsyncBoundary>
      {directorOpen && <RoleDirectorDialog onClose={() => setDirectorOpen(false)} />}
    </section>
  );
}
