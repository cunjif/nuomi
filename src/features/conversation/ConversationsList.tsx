import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { Button } from "../../components/ui/Button";
import { Dialog } from "../../components/ui/Dialog";
import { ipc } from "../../lib/ipc/client";
import { describeError } from "../../i18n";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";
import { formatRelativeTime, type RelativeTimeLocale } from "../../lib/format/relativeTime";
import type { ConversationKind } from "../../lib/conversation/kinds";

/** Kind icon for list rows. */
function kindIcon(kind: string): string {
  switch (kind) {
    case "chat": return "💬";
    case "group": return "👥";
    case "background": return "⚙";
    case "scheduled": return "⏰";
    default: return "💬";
  }
}

const FILTER_TABS: Array<{ key: ConversationKind | "all"; labelKey: string }> = [
  { key: "all", labelKey: "conversation.filterAll" },
  { key: "chat", labelKey: "conversation.filterChat" },
  { key: "group", labelKey: "conversation.filterGroup" },
  { key: "background", labelKey: "conversation.filterBackground" },
  { key: "scheduled", labelKey: "conversation.filterScheduled" },
];

/**
 * Upgraded session list with kind filtering tabs, kind icons, and agent badges.
 * Replaces SessionsList in the LeftRail.
 */
export function ConversationsList(): ReactNode {
  const { t, i18n } = useTranslation();
  const qc = useQueryClient();
  const selectedSessionId = useUiStore((s) => s.selectedSessionId);
  const selectSession = useUiStore((s) => s.selectSession);
  const setView = useUiStore((s) => s.setView);
  const [filter, setFilter] = useState<ConversationKind | "all">("all");
  const [confirmNoProvider, setConfirmNoProvider] = useState(false);
  const locale: RelativeTimeLocale = i18n.language === "en" ? "en" : "zh-CN";

  const providersQuery = useQuery({
    queryKey: ["providers"],
    queryFn: () => ipc.listProviders(),
    staleTime: 30_000,
  });

  const conversationsQuery = useQuery({
    queryKey: ["conversations", filter],
    queryFn: () => ipc.listConversations(filter === "all" ? null : filter),
    refetchInterval: 15_000,
  });

  const resumeMut = useMutation({
    mutationFn: ipc.resumeSession,
    onError: (e) => toast.error(`${t("sessions.resumeFailed")}: ${describeError(e)}`),
  });

  const createMut = useMutation({
    mutationFn: () => ipc.createConversation({ kind: "chat", title: null, agent: null, teamId: null }),
    onSuccess: (session) => {
      void qc.invalidateQueries({ queryKey: ["conversations"] });
      selectSession(session.id);
      // Keep the kernel's active session in step with the selection:
      // anything still reading `kernel.session_id()` (task creation,
      // capability routing) would otherwise point at the old session.
      resumeMut.mutate(session.id);
    },
    onError: (e) => toast.error(describeError(e)),
  });

  const create = (): void => {
    setConfirmNoProvider(false);
    createMut.mutate();
  };

  const conversations = conversationsQuery.data ?? [];

  return (
    <section aria-label={t("sessions.heading")} className="flex h-full flex-col p-2">
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-xs font-semibold uppercase tracking-wide text-ink-muted">{t("sessions.heading")}</h2>
        <button
          type="button"
          onClick={() => {
            // Without a Provider the conversation cannot call any model,
            // so guide the user to Settings instead of creating an empty
            // session that would immediately be unusable.
            if ((providersQuery.data ?? []).length === 0) {
              setConfirmNoProvider(true);
              return;
            }
            create();
          }}
          disabled={createMut.isPending}
          className="pixel-fill-accent px-2 py-0.5 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
        >
          {t("sessions.newSession")}
        </button>
      </div>
      {/* Kind filter tabs */}
      <div className="mb-1 flex gap-0.5" role="tablist">
        {FILTER_TABS.map((tab) => (
          <button
            key={tab.key}
            type="button"
            role="tab"
            aria-selected={filter === tab.key}
            onClick={() => setFilter(tab.key)}
            className={`rounded px-1.5 py-0.5 text-xs ${
              filter === tab.key
                ? "bg-surface-overlay text-ink"
                : "text-ink-muted hover:bg-surface-overlay"
            }`}
          >
            {t(tab.labelKey)}
          </button>
        ))}
      </div>
      <AsyncBoundary
        isLoading={conversationsQuery.isLoading}
        error={conversationsQuery.error}
        isEmpty={conversations.length === 0}
        onRetry={() => void conversationsQuery.refetch()}
      >
        <ul className="flex flex-col gap-0.5">
          {conversations.map((conv) => (
            <li key={conv.id}>
              <button
                type="button"
                onClick={() => {
                  selectSession(conv.id);
                  resumeMut.mutate(conv.id);
                }}
                aria-current={selectedSessionId === conv.id ? "true" : undefined}
                title={conv.title}
                className={`flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-sm focus-visible:ring-2 focus-visible:ring-ink-accent ${
                  selectedSessionId === conv.id
                    ? "bg-surface-overlay text-ink"
                    : "text-ink-muted hover:bg-surface-overlay"
                }`}
              >
                <span aria-hidden="true" className="shrink-0 text-xs">{kindIcon(conv.kind)}</span>
                <span className="min-w-0 flex-1 truncate">{conv.title}</span>
                {conv.agent && (
                  <span className="shrink-0 rounded bg-ink-muted/20 px-1 text-[10px] text-ink-muted">
                    {conv.agent.name}
                  </span>
                )}
                <span className="shrink-0 text-xs tabular-nums">
                  {formatRelativeTime(conv.createdAt, Date.now(), locale)}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </AsyncBoundary>

      <Dialog
        open={confirmNoProvider}
        title={t("conversation.noProviderTitle")}
        onClose={() => setConfirmNoProvider(false)}
        footer={
          <>
            <Button variant="ghost" size="sm" onClick={() => setConfirmNoProvider(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="solid"
              size="sm"
              onClick={() => {
                setConfirmNoProvider(false);
                setView("settings");
              }}
            >
              {t("chat.goToSettings")}
            </Button>
          </>
        }
      >
        {t("conversation.noProviderConfirm")}
      </Dialog>
    </section>
  );
}
