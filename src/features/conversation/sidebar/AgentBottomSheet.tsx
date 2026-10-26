import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../../lib/ipc/client";
import { AsyncBoundary } from "../../../components/ui/AsyncBoundary";
import { flavorLabel } from "../../../lib/conversation/agentResolve";
import { toast } from "../../../lib/store/toastStore";
import { i18n } from "../../../i18n";

export interface AgentBottomSheetProps {
  agentKind: string;
  agentId: string;
  onClose: () => void;
  /** Called when user clicks "Remove member". Only shown when canRemove=true. */
  onRemove?: () => void;
  /** Whether the "Remove member" button is available (group chat with >1 participants). */
  canRemove?: boolean;
  /** Session whose conversation detail (binding/policies) the panel shows
   * (chat_rich_bind 5.5); null hides the session-info section. */
  sessionId?: string | null;
}

type PolicyValue = "authorize" | "deny" | "unset";

/**
 * Conversation-detail bottom sheet (chat_rich_bind spec 5.5.1): session
 * info (workspace + bound work directory with derived availability) +
 * per-participant default authorization policy (three-state, applied
 * immediately via `set_agent_auth_policy`, reverted on failure) + the
 * pre-existing per-agent detail (role / responsibility / bound model /
 * provider / CLI agent / remove member). Slides up from the sidebar's
 * bottom half, 250ms transition.
 */
export function AgentBottomSheet({ agentKind, agentId, onClose, onRemove, canRemove, sessionId }: AgentBottomSheetProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  /** Policy in flight per participant (keyed by role id) — guards the
   * optimistic three-state switch and reverts on save failure. */
  const [pendingPolicy, setPendingPolicy] = useState<Record<string, PolicyValue>>({});

  const detailQuery = useQuery({
    queryKey: ["agentDetail", agentKind, agentId],
    queryFn: () => ipc.getAgentDetail(agentKind, agentId),
    staleTime: 60_000,
  });

  const conversationQuery = useQuery({
    queryKey: ["conversationDetail", sessionId],
    queryFn: () => ipc.getConversationDetail(sessionId ?? ""),
    enabled: sessionId !== null && sessionId !== undefined,
    staleTime: 30_000,
  });

  const applyPolicy = async (roleId: string, next: PolicyValue): Promise<void> => {
    setPendingPolicy((prev) => ({ ...prev, [roleId]: next }));
    try {
      await ipc.setAgentAuthPolicy(roleId, next);
      void qc.invalidateQueries({ queryKey: ["conversationDetail", sessionId] });
    } catch {
      // Save failed: revert the control to the persisted value and toast.
      toast.error(i18n.t("conversation.conversationDetail.policySaveFailed"));
      setPendingPolicy((prev) => {
        const copy = { ...prev };
        delete copy[roleId];
        return copy;
      });
    } finally {
      setPendingPolicy((prev) => {
        const copy = { ...prev };
        delete copy[roleId];
        return copy;
      });
    }
  };

  const conversation = conversationQuery.data;

  return (
    <div
      className="absolute inset-x-0 bottom-0 z-20 flex max-h-[60%] flex-col rounded-t border-t border-ink-muted/40 bg-surface-raised shadow-lg transition-transform duration-[250ms] ease-out"
      role="dialog"
      aria-label={t("conversation.conversationDetail.title")}
    >
      <div className="flex shrink-0 items-center justify-between border-b border-ink-muted/30 px-3 py-2">
        <span className="text-sm font-medium text-ink">{t("conversation.conversationDetail.title")}</span>
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
        {conversation !== undefined && (
          <section data-testid="conversation-info" className="mb-3 space-y-2 rounded border border-ink-muted/30 bg-surface p-2.5">
            <div className="text-xs font-medium uppercase tracking-wide text-ink-muted">
              {t("conversation.conversationDetail.sessionInfo")}
            </div>
            <div>
              <div className="text-xs text-ink-muted">{t("conversation.conversationDetail.workspace")}</div>
              <div className="text-sm text-ink">{conversation.workspace?.name ?? conversation.workspace?.label ?? t("conversation.agentDetail.notSet")}</div>
            </div>
            <div>
              <div className="text-xs text-ink-muted">{t("conversation.conversationDetail.workDirectory")}</div>
              {conversation.workDirectory === null ? (
                <div className="text-sm text-ink-muted">{t("conversation.conversationDetail.noBinding")}</div>
              ) : (
                <div className="text-sm text-ink">
                  <span className={conversation.workDirectory.available ? "" : "text-ink-muted line-through"} title={conversation.workDirectory.absPath}>
                    {conversation.workDirectory.displayName}
                  </span>
                  {!conversation.workDirectory.available && (
                    <span className="ml-1.5 rounded bg-state-danger/15 px-1 py-0.5 text-[10px] text-state-danger" data-testid="directory-unavailable">
                      {t("conversation.conversationDetail.directoryUnavailable")}
                    </span>
                  )}
                </div>
              )}
            </div>
            <div>
              <div className="text-xs text-ink-muted">{t("conversation.conversationDetail.participants")}</div>
              <ul className="mt-0.5 space-y-1">
                {conversation.participants.map((participant) => (
                  <li key={`${participant.kind}:${participant.id}`} className="flex items-center justify-between gap-2">
                    <span className="min-w-0 truncate text-sm text-ink" title={participant.id}>
                      {participant.name}
                    </span>
                    {participant.kind === "role" ? (
                      <span className="flex shrink-0 items-center gap-0.5" data-testid={`policy-${participant.id}`}>
                        {(["authorize", "deny", "unset"] as const).map((value) => {
                          const current = pendingPolicy[participant.id] ?? participant.authPolicy;
                          const active = current === value;
                          return (
                            <button
                              key={value}
                              type="button"
                              disabled={pendingPolicy[participant.id] !== undefined}
                              onClick={() => void applyPolicy(participant.id, value)}
                              className={`rounded px-1.5 py-0.5 text-[10px] transition-colors ${
                                active
                                  ? value === "deny"
                                    ? "bg-state-danger/20 text-state-danger"
                                    : value === "authorize"
                                      ? "bg-ink-accent/20 text-ink-accent"
                                      : "bg-surface-overlay text-ink-muted"
                                  : "text-ink-muted hover:bg-surface-overlay"
                              }`}
                            >
                              {value === "authorize"
                                ? t("conversation.conversationDetail.policyAuthorize")
                                : value === "deny"
                                  ? t("conversation.conversationDetail.policyDeny")
                                  : t("conversation.conversationDetail.policyUnset")}
                            </button>
                          );
                        })}
                      </span>
                    ) : (
                      <span className="shrink-0 text-[10px] text-ink-muted">
                        {t("conversation.conversationDetail.defaultAuthPolicy")}: —
                      </span>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          </section>
        )}
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
                  className="w-full rounded border border-ink-muted/30 px-3 py-1.5 text-sm text-ink-muted hover:border-state-danger hover:text-state-danger"
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
