import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { TeamDto, TeamTopologyDto } from "../../lib/ipc/bindings.gen";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { TeamForm } from "./TeamForm";

const TOPOLOGY_LABEL_KEYS: Record<TeamTopologyDto, string> = {
  pipeline: "settings.teams.topologyPipeline",
  router: "settings.teams.topologyRouter",
  group_chat: "settings.teams.topologyGroupChat",
};

interface TeamRowProps {
  team: TeamDto;
  confirming: boolean;
  onAskDelete: () => void;
  onConfirmDelete: () => void;
  deletePending: boolean;
}

/** One team row: name + topology badge + member count + two-step delete. */
function TeamRow({ team, confirming, onAskDelete, onConfirmDelete, deletePending }: TeamRowProps): ReactNode {
  const { t } = useTranslation();
  return (
    <li className="rounded border border-ink-muted/40 bg-surface-raised p-3 text-xs">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm font-medium text-ink">{team.name}</span>
        <span className="rounded bg-ink-accent/20 px-1.5 py-0.5 text-[10px] text-ink-accent">
          {t(TOPOLOGY_LABEL_KEYS[team.topology])}
        </span>
        <span className="text-ink-muted">
          {t("settings.teams.memberCount", { count: team.memberRoleIds.length })}
        </span>
      </div>
      <div className="mt-1.5 flex gap-2">
        {confirming ? (
          <button
            type="button"
            onClick={onConfirmDelete}
            disabled={deletePending}
            aria-label={`${t("settings.teams.deleteConfirm")} ${team.name}`}
            className="rounded border border-state-danger px-2 py-0.5 text-xs text-state-danger hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
          >
            {t("settings.teams.deleteConfirm")}
          </button>
        ) : (
          <button
            type="button"
            onClick={onAskDelete}
            aria-label={`${t("common.delete")} ${team.name}`}
            className="rounded border border-ink-muted px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("common.delete")}
          </button>
        )}
      </div>
    </li>
  );
}

/** Settings section listing teams with topology badges and delete actions. */
export function TeamsSection(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const query = useQuery({ queryKey: ["teams"], queryFn: ipc.listTeams });
  /** teamId awaiting a second click on Delete (two-step confirm, keyboard friendly) */
  const [confirmingId, setConfirmingId] = useState<string | null>(null);

  const deleteMut = useMutation({
    mutationFn: (teamId: string) => ipc.deleteTeam(teamId),
    onSuccess: () => {
      setConfirmingId(null);
      void qc.invalidateQueries({ queryKey: ["teams"] });
      toast.success(t("settings.teams.deleted"));
    },
    onError: (e) => toast.error(`${t("settings.teams.deleteFailed")}: ${describeError(e)}`),
  });

  return (
    <section aria-label={t("settings.teams.heading")} className="mb-3">
      <h3 className="mb-2 text-sm font-semibold text-ink">{t("settings.teams.heading")}</h3>
      <TeamForm />
      <AsyncBoundary
        isLoading={query.isLoading}
        error={query.error}
        isEmpty={(query.data?.length ?? 0) === 0}
        emptyLabel={t("settings.teams.empty")}
        onRetry={() => void query.refetch()}
      >
        <ul className="flex flex-col gap-2">
          {(query.data ?? []).map((team) => (
            <TeamRow
              key={team.id}
              team={team}
              confirming={confirmingId === team.id}
              onAskDelete={() => setConfirmingId(team.id)}
              onConfirmDelete={() => deleteMut.mutate(team.id)}
              deletePending={deleteMut.isPending}
            />
          ))}
        </ul>
      </AsyncBoundary>
    </section>
  );
}
