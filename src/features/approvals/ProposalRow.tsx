import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ProposalDto, TeamTopologyDto } from "../../lib/ipc/bindings.gen";
import { WorkspaceBadge } from "../conversation/WorkspaceBadge";
import type { ViewScope } from "../common/ViewScopeToggle";

interface ProposalRowProps {
  proposal: ProposalDto;
  scope: ViewScope;
  onResolve: (proposalId: string, approved: boolean) => void;
  pending: boolean;
}

function topologyLabel(topology: TeamTopologyDto, t: ReturnType<typeof useTranslation>["t"]): string {
  if (topology === "pipeline") return t("teams.topologyPipeline");
  if (topology === "router") return t("teams.topologyRouter");
  return t("teams.topologyGroupChat");
}

function sourceLabel(source: string, t: ReturnType<typeof useTranslation>["t"]): string {
  if (source === "scheduled") return t("approvals.proposal.sourceScheduled");
  if (source === "idle") return t("approvals.proposal.sourceIdle");
  return t("approvals.proposal.sourceManual");
}

/** One inbox item of the proposal family: the planned team (members,
 * topology, rationale) + the tasks it would dispatch onto — approve builds
 * exactly what the preview showed, deny discards it. */
export function ProposalRow({ proposal, scope, onResolve, pending }: ProposalRowProps): ReactNode {
  const { t } = useTranslation();
  return (
    <li className="rounded border border-ink-muted/40 bg-surface-raised p-3">
      <div className="flex flex-wrap items-center gap-2">
        <p className="text-sm font-semibold text-ink-accent">{t("approvals.proposal.heading")}</p>
        <span className="rounded-full border border-ink-muted/60 px-2 py-0.5 text-xs text-ink-muted">
          {sourceLabel(proposal.source, t)}
        </span>
        <span className="text-xs text-ink-muted">
          {topologyLabel(proposal.plan.topology, t)}
        </span>
        {scope === "all" && (
          <WorkspaceBadge
            workspaceId={proposal.workspaceId}
            workspaceLabel={proposal.workspaceLabel}
          />
        )}
      </div>
      <ul className="mt-2 flex flex-wrap gap-1.5" aria-label={t("approvals.proposal.members")}>
        {proposal.plan.members.map((member) => (
          <li
            key={`${member.kind}-${member.refId}`}
            className="rounded-full bg-surface px-2 py-0.5 text-xs text-ink"
            title={member.systemPrompt ?? member.refId}
          >
            <span className="font-mono text-[10px] text-ink-muted">{member.kind}</span>{" "}
            {member.roleName}
          </li>
        ))}
      </ul>
      <p className="mt-2 text-xs text-ink-muted">{proposal.plan.rationale}</p>
      <p className="mt-1 text-xs text-ink-muted">
        {t("approvals.proposal.taskCount", { count: proposal.plan.taskIds.length })}
        {proposal.plan.maxRounds != null &&
          ` · ${t("approvals.proposal.maxRounds", { count: proposal.plan.maxRounds })}`}
      </p>
      <div className="mt-2 flex gap-2">
        <button
          type="button"
          disabled={pending}
          onClick={() => onResolve(proposal.id, true)}
          className="rounded bg-state-ok px-3 py-1 text-xs font-medium text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
        >
          {t("approvals.approve")}
        </button>
        <button
          type="button"
          disabled={pending}
          onClick={() => onResolve(proposal.id, false)}
          className="rounded border border-state-danger px-3 py-1 text-xs font-medium text-state-danger hover:bg-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
        >
          {t("approvals.deny")}
        </button>
      </div>
    </li>
  );
}
