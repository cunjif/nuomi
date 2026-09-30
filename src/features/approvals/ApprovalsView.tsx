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
import { ViewScopeToggle } from "../common/ViewScopeToggle";
import { useViewScope } from "../common/useViewScope";

/**
 * Approvals inbox: polled as a safety net and invalidated immediately by
 * `approval.*` events on the global domain channel.
 */
export function ApprovalsView(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const focusedWorkspaceId = useUiStore((s) => s.focusedWorkspaceId);
  const { scope, setScope } = useViewScope("approvals");
  const workspaceFilter = scope === "focused" ? focusedWorkspaceId : null;
  const approvalsQuery = useQuery({
    queryKey: ["approvals", workspaceFilter],
    queryFn: () => ipc.listPendingApprovals(workspaceFilter),
    refetchInterval: 5000,
  });

  const resolveMut = useMutation({
    mutationFn: ({ id, approved }: { id: string; approved: boolean }) => ipc.resolveApproval(id, approved),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["approvals"] }),
    onError: (e) => toast.error(`${t("approvals.resolveFailed")}: ${describeError(e)}`),
  });

  useDomainEvents([DOMAIN_CHANNEL], (batch) => {
    if (batch.some((e) => e.type.startsWith("approval."))) {
      void qc.invalidateQueries({ queryKey: ["approvals"] });
    }
  });

  return (
    <div className="h-full overflow-y-auto p-3">
      <div className="mb-2 flex items-center gap-3">
        <h2 className="text-sm font-semibold">{t("approvals.heading")}</h2>
        <ViewScopeToggle surface="approvals" scope={scope} onScopeChange={setScope} />
      </div>
      <AsyncBoundary
        isLoading={approvalsQuery.isLoading}
        error={approvalsQuery.error}
        isEmpty={(approvalsQuery.data?.length ?? 0) === 0}
        emptyLabel={t("approvals.empty")}
        onRetry={() => void approvalsQuery.refetch()}
      >
        <ul className="flex flex-col gap-2">
          {(approvalsQuery.data ?? []).map((approval) => (
            <ApprovalRow
              key={approval.id}
              approval={approval}
              scope={scope}
              pending={resolveMut.isPending}
              onResolve={(id, approved) => resolveMut.mutate({ id, approved })}
            />
          ))}
        </ul>
      </AsyncBoundary>
    </div>
  );
}
