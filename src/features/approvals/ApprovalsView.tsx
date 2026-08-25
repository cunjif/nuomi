import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { DOMAIN_CHANNEL } from "../../lib/events/types";
import { useDomainEvents } from "../../lib/events/useDomainEvents";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { ApprovalRow } from "./ApprovalRow";

/**
 * Approvals inbox: polled as a safety net and invalidated immediately by
 * `approval.*` events on the global domain channel.
 */
export function ApprovalsView(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const approvalsQuery = useQuery({
    queryKey: ["approvals"],
    queryFn: ipc.listPendingApprovals,
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
      <h2 className="mb-2 text-sm font-semibold">{t("approvals.heading")}</h2>
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
              pending={resolveMut.isPending}
              onResolve={(id, approved) => resolveMut.mutate({ id, approved })}
            />
          ))}
        </ul>
      </AsyncBoundary>
    </div>
  );
}
