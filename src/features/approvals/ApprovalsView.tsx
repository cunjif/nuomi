import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { DOMAIN_CHANNEL } from "../../lib/events/types";
import { useDomainEvents } from "../../lib/events/useDomainEvents";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";
import { ApprovalRow } from "./ApprovalRow";
import { ProposalRow } from "./ProposalRow";
import { ViewScopeToggle } from "../common/ViewScopeToggle";
import { useViewScope } from "../common/useViewScope";

/**
 * Approvals inbox: polled as a safety net and invalidated immediately by
 * `approval.*` / `proposal.*` events on the global domain channel. Holds two
 * row families — sensitive-tool approvals and advisory team-plan proposals
 * (P2-F 组 2) — in one list.
 */
export function ApprovalsView({ workspaceId }: { workspaceId: string | null }): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const activeWorkspaceId = useUiStore((s) => s.activeWorkspaceId);
  const { scope, setScope } = useViewScope("approvals");
  const effectiveWorkspaceId = workspaceId ?? activeWorkspaceId;
  const workspaceFilter = scope === "focused" ? effectiveWorkspaceId : null;
  const approvalsQuery = useQuery({
    queryKey: ["approvals", workspaceFilter],
    queryFn: () => ipc.listPendingApprovals(workspaceFilter),
    refetchInterval: 5000,
  });
  const proposalsQuery = useQuery({
    queryKey: ["proposals", workspaceFilter],
    queryFn: () => ipc.listProposals(workspaceFilter),
    refetchInterval: 5000,
  });

  const resolveApprovalMut = useMutation({
    mutationFn: ({ id, approved }: { id: string; approved: boolean }) =>
      ipc.resolveApproval(id, approved),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["approvals"] }),
    onError: (e) => toast.error(`${t("approvals.resolveFailed")}: ${describeError(e)}`),
  });
  const resolveProposalMut = useMutation({
    mutationFn: ({ id, approved }: { id: string; approved: boolean }) =>
      ipc.resolveProposal(id, approved),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["proposals"] }),
    onError: (e) => toast.error(`${t("approvals.resolveFailed")}: ${describeError(e)}`),
  });

  useDomainEvents([DOMAIN_CHANNEL], (batch) => {
    if (
      batch.some(
        (e) => e.type.startsWith("approval.") || e.type.startsWith("proposal."),
      )
    ) {
      void qc.invalidateQueries({ queryKey: ["approvals"] });
      void qc.invalidateQueries({ queryKey: ["proposals"] });
    }
  });

  const approvals = approvalsQuery.data ?? [];
  const proposals = proposalsQuery.data ?? [];

  return (
    <div className="h-full overflow-y-auto p-3">
      <div className="mb-2 flex items-center gap-3">
        <h2 className="text-sm font-semibold">{t("approvals.heading")}</h2>
        <ViewScopeToggle surface="approvals" scope={scope} onScopeChange={setScope} />
      </div>
      <AsyncBoundary
        isLoading={approvalsQuery.isLoading || proposalsQuery.isLoading}
        error={approvalsQuery.error ?? proposalsQuery.error}
        isEmpty={approvals.length === 0 && proposals.length === 0}
        emptyLabel={t("approvals.empty")}
        onRetry={() => {
          void approvalsQuery.refetch();
          void proposalsQuery.refetch();
        }}
      >
        <ul className="flex flex-col gap-2">
          {approvals.map((approval) => (
            <ApprovalRow
              key={approval.id}
              approval={approval}
              scope={scope}
              pending={resolveApprovalMut.isPending}
              onResolve={(id, approved) => resolveApprovalMut.mutate({ id, approved })}
            />
          ))}
          {proposals.map((proposal) => (
            <ProposalRow
              key={proposal.id}
              proposal={proposal}
              scope={scope}
              pending={resolveProposalMut.isPending}
              onResolve={(id, approved) => resolveProposalMut.mutate({ id, approved })}
            />
          ))}
        </ul>
      </AsyncBoundary>
    </div>
  );
}
