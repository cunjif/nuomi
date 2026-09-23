import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../../lib/ipc/client";
import { AsyncBoundary } from "../../../components/ui/AsyncBoundary";
import { flavorLabel } from "../../../lib/conversation/agentResolve";

export interface AgentBottomSheetProps {
  agentKind: string;
  agentId: string;
  onClose: () => void;
  /** Called when user clicks "Remove member". Only shown when canRemove=true. */
  onRemove?: () => void;
  /** Whether the "Remove member" button is available (group chat with >1 participants). */
  canRemove?: boolean;
}

/**
 * Bottom sheet showing a single agent's detail (role, responsibility,
 * bound model, provider). Slides up from the bottom of the sidebar,
 * covering the lower half. 250ms transition.
 * ADR 0013: "Remove member" button at the bottom (group chat only).
 */
export function AgentBottomSheet({ agentKind, agentId, onClose, onRemove, canRemove }: AgentBottomSheetProps): ReactNode {
  const { t } = useTranslation();

  const detailQuery = useQuery({
    queryKey: ["agentDetail", agentKind, agentId],
    queryFn: () => ipc.getAgentDetail(agentKind, agentId),
    staleTime: 60_000,
  });

  return (
    <div
      className="absolute inset-x-0 bottom-0 z-20 flex max-h-[60%] flex-col rounded-t border-t border-ink-muted/40 bg-surface-raised shadow-lg transition-transform duration-[250ms] ease-out"
      role="dialog"
      aria-label={t("conversation.agentDetail.title")}
    >
      <div className="flex shrink-0 items-center justify-between border-b border-ink-muted/30 px-3 py-2">
        <span className="text-sm font-medium text-ink">{t("conversation.agentDetail.title")}</span>
        <button
          type="button"
          onClick={onClose}
          className="text-ink-muted hover:text-ink"
          aria-label={t("common.close")}
        >
          ✕
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-3">
        <AsyncBoundary
          isLoading={detailQuery.isLoading}
          error={detailQuery.error}
          isEmpty={false}
          onRetry={() => void detailQuery.refetch()}
        >
          {detailQuery.data && (
            <div className="space-y-3">
              <div>
                <div className="text-xs text-ink-muted">{t("conversation.agentDetail.name")}</div>
                <div className="text-sm text-ink">{detailQuery.data.name}</div>
              </div>
              <div>
                <div className="text-xs text-ink-muted">{t("conversation.agentDetail.role")}</div>
                <div className="text-sm text-ink">
                  {detailQuery.data.role ?? t("conversation.agentDetail.notSet")}
                </div>
              </div>
              <div>
                <div className="text-xs text-ink-muted">{t("conversation.agentDetail.responsibility")}</div>
                <div className="text-sm text-ink">
                  {detailQuery.data.responsibility ?? t("conversation.agentDetail.notSet")}
                </div>
              </div>
              {detailQuery.data.bindingKind === "cli" ? (
                <>
                  <div>
                    <div className="text-xs text-ink-muted">{t("conversation.agentDetail.cliAgent")}</div>
                    <div className="text-sm text-ink">
                      {detailQuery.data.cliAgentName ?? t("conversation.agentDetail.notSet")}
                      {detailQuery.data.cliAgentFlavor && (
                        <span className="ml-1 text-xs text-ink-muted">
                          ({flavorLabel(detailQuery.data.cliAgentFlavor)})
                        </span>
                      )}
                    </div>
                  </div>
                  <div>
                    <div className="text-xs text-ink-muted">{t("conversation.agentDetail.boundModel")}</div>
                    <div className="text-sm text-ink">
                      {detailQuery.data.cliAgentModel ?? t("conversation.agentDetail.modelFromCli")}
                    </div>
                  </div>
                </>
              ) : (
                <>
                  <div>
                    <div className="text-xs text-ink-muted">{t("conversation.agentDetail.provider")}</div>
                    <div className="text-sm text-ink">
                      {detailQuery.data.provider ?? t("conversation.agentDetail.notSet")}
                    </div>
                  </div>
                  <div>
                    <div className="text-xs text-ink-muted">{t("conversation.agentDetail.boundModel")}</div>
                    <div className="text-sm text-ink">
                      {detailQuery.data.boundModel ?? t("conversation.agentDetail.notSet")}
                    </div>
                  </div>
                </>
              )}
              {canRemove && onRemove && (
                <button
                  type="button"
                  onClick={onRemove}
                  className="w-full rounded border border-ink-muted/30 px-3 py-1.5 text-sm text-ink-muted hover:border-red-500 hover:text-red-500"
                >
                  {t("conversation.agentDetail.removeMember")}
                </button>
              )}
            </div>
          )}
        </AsyncBoundary>
      </div>
    </div>
  );
}
