import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ApprovalDto } from "../../lib/ipc/bindings.gen";
import { WorkspaceBadge } from "../conversation/WorkspaceBadge";
import type { ViewScope } from "../common/ViewScopeToggle";

interface ApprovalRowProps {
  approval: ApprovalDto;
  scope: ViewScope;
  onResolve: (approvalId: string, approved: boolean) => void;
  pending: boolean;
}

/** One inbox item: tool name + arguments preview + approve/deny actions. */
export function ApprovalRow({ approval, scope, onResolve, pending }: ApprovalRowProps): ReactNode {
  const { t } = useTranslation();
  return (
    <li className="rounded border border-ink-muted/40 bg-surface-raised p-3">
      <div className="flex items-center gap-2">
        <p className="font-mono text-sm text-ink-accent">{approval.toolName}</p>
        {scope === "all" && (
          <WorkspaceBadge workspaceId={approval.workspaceId} workspaceRootPath={approval.workspaceRootPath} />
        )}
      </div>
      <pre className="mt-1 max-h-24 overflow-auto rounded bg-surface p-2 font-mono text-xs text-ink-muted">
        {approval.argumentsJson}
      </pre>
      <div className="mt-2 flex gap-2">
        <button
          type="button"
          disabled={pending}
          onClick={() => onResolve(approval.id, true)}
          className="rounded bg-state-ok px-3 py-1 text-xs font-medium text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
        >
          {t("approvals.approve")}
        </button>
        <button
          type="button"
          disabled={pending}
          onClick={() => onResolve(approval.id, false)}
          className="rounded border border-state-danger px-3 py-1 text-xs font-medium text-state-danger hover:bg-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
        >
          {t("approvals.deny")}
        </button>
      </div>
    </li>
  );
}
