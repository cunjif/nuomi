import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { Dialog } from "../../components/ui/Dialog";
import { ipc } from "../../lib/ipc/client";
import { describeError } from "../../i18n";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";
import { formatRelativeTime, type RelativeTimeLocale } from "../../lib/format/relativeTime";
import type { ConversationKind } from "../../lib/conversation/kinds";
import { NewConversationDialog } from "./new/NewConversationDialog";
import { WorkspaceFilter } from "./WorkspaceFilter";
import { WorkspaceBadge } from "./WorkspaceBadge";

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

type ConfirmAction =
  | { type: "delete"; sessionId: string; title: string }
  | { type: "clear" }
  | { type: "batch"; ids: string[] };

export function ConversationsList(): ReactNode {
  const { t, i18n } = useTranslation();
  const qc = useQueryClient();
  const selectedSessionId = useUiStore((s) => s.selectedSessionId);
  const selectSession = useUiStore((s) => s.selectSession);
  const [filter, setFilter] = useState<ConversationKind | "all">("all");
  const workspaceFilter = useUiStore((s) => s.conversationWorkspaceFilter);
  const [dialogKind, setDialogKind] = useState<ConversationKind | null>(null);
  const [batchMode, setBatchMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState<ConfirmAction | null>(null);
  const locale: RelativeTimeLocale = i18n.language === "en" ? "en" : "zh-CN";

  const conversationsQuery = useQuery({
    queryKey: ["conversations", filter, workspaceFilter],
    queryFn: () => ipc.listConversations(filter === "all" ? null : filter, workspaceFilter === "all" ? null : workspaceFilter),
    refetchInterval: 15_000,
  });

  const workspacesQuery = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => ipc.listWorkspaces(),
    staleTime: 30_000,
  });

  const workspacePathMap = new Map((workspacesQuery.data ?? []).map((ws) => [ws.id, ws.rootPath]));

  const resumeMut = useMutation({
    mutationFn: ipc.resumeSession,
    onError: (e) => toast.error(`${t("sessions.resumeFailed")}: ${describeError(e)}`),
  });

  const deleteMut = useMutation({
    mutationFn: (sessionId: string) => ipc.deleteConversation(sessionId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["conversations"] });
      toast.success(t("conversation.deleted"));
    },
    onError: (e) => toast.error(`${t("conversation.deleteFailed")}: ${describeError(e)}`),
  });

  const clearMut = useMutation({
    mutationFn: () => ipc.clearConversations(),
    onSuccess: (count) => {
      void qc.invalidateQueries({ queryKey: ["conversations"] });
      toast.success(t("conversation.cleared", { count }));
    },
    onError: (e) => toast.error(`${t("conversation.clearFailed")}: ${describeError(e)}`),
  });

  const conversations = conversationsQuery.data ?? [];

  const toggleSelect = (id: string): void => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const allSelected = conversations.length > 0 && selectedIds.size === conversations.length;

  const toggleAll = (): void => {
    if (allSelected) setSelectedIds(new Set());
    else setSelectedIds(new Set(conversations.map((c) => c.id)));
  };

  const exitBatchMode = (): void => {
    setBatchMode(false);
    setSelectedIds(new Set());
  };

  const handleConfirm = async (): Promise<void> => {
    if (!confirm) return;
    setConfirm(null);
    if (confirm.type === "delete") {
      await deleteMut.mutateAsync(confirm.sessionId);
      if (selectedSessionId === confirm.sessionId) selectSession(null);
    } else if (confirm.type === "clear") {
      await clearMut.mutateAsync();
      selectSession(null);
    } else if (confirm.type === "batch") {
      for (const id of confirm.ids) {
        await deleteMut.mutateAsync(id);
      }
      if (confirm.ids.includes(selectedSessionId ?? "")) selectSession(null);
      exitBatchMode();
    }
  };

  const confirmTitle = confirm
    ? confirm.type === "delete"
      ? t("conversation.confirmDeleteTitle")
      : confirm.type === "clear"
        ? t("conversation.confirmClearTitle")
        : t("conversation.confirmBatchDeleteTitle")
    : "";

  const confirmMessage = confirm
    ? confirm.type === "delete"
      ? t("conversation.confirmDeleteMessage", { title: confirm.title })
      : confirm.type === "clear"
        ? t("conversation.confirmClearMessage")
        : t("conversation.confirmBatchDeleteMessage", { count: confirm.ids.length })
    : "";

  return (
    <section aria-label={t("sessions.heading")} className="flex h-full flex-col p-2">
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-xs font-semibold uppercase tracking-wide text-ink-muted">{t("sessions.heading")}</h2>
        <div className="flex gap-1">
          {!batchMode && (
            <>
              <button
                type="button"
                onClick={() => setBatchMode(true)}
                className="rounded border border-ink-muted/40 px-1.5 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay"
              >
                {t("conversation.batch")}
              </button>
              <button
                type="button"
                onClick={() => setConfirm({ type: "clear" })}
                disabled={conversations.length === 0}
                className="rounded border border-ink-muted/40 px-1.5 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay disabled:opacity-30"
              >
                {t("conversation.clear")}
              </button>
            </>
          )}
          <button
            type="button"
            onClick={() => setDialogKind("chat")}
            className="pixel-fill-accent px-2 py-0.5 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("sessions.newSession")}
          </button>
        </div>
      </div>

      {batchMode && (
        <div className="mb-1 flex items-center justify-between rounded bg-surface-overlay px-2 py-1">
          <label className="flex items-center gap-1.5 text-xs text-ink">
            <input
              type="checkbox"
              checked={allSelected}
              onChange={toggleAll}
              className="accent-ink-accent"
            />
            {t("conversation.selectAll")}
          </label>
          <div className="flex gap-1">
            <button
              type="button"
              onClick={exitBatchMode}
              className="rounded border border-ink-muted/40 px-1.5 py-0.5 text-xs text-ink-muted hover:bg-surface-raised"
            >
              {t("conversation.cancel")}
            </button>
            <button
              type="button"
              onClick={() => {
                if (selectedIds.size > 0) {
                  setConfirm({ type: "batch", ids: [...selectedIds] });
                }
              }}
              disabled={selectedIds.size === 0}
              className="rounded bg-red-600 px-1.5 py-0.5 text-xs text-white disabled:opacity-30"
            >
              {t("conversation.deleteSelected", { count: selectedIds.size })}
            </button>
          </div>
        </div>
      )}

      {!batchMode && (
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
      )}

      <div className="min-h-0 flex-1 overflow-y-auto">
      <AsyncBoundary
        isLoading={conversationsQuery.isLoading}
        error={conversationsQuery.error}
        isEmpty={conversations.length === 0}
        onRetry={() => void conversationsQuery.refetch()}
      >
        <ul className="flex flex-col gap-0.5">
          {conversations.map((conv) => (
            <li key={conv.id}>
              <div
                className={`group flex w-full items-center gap-1.5 rounded px-2 py-1 text-left text-sm ${
                  batchMode
                    ? ""
                    : selectedSessionId === conv.id
                      ? "bg-surface-overlay text-ink"
                      : "text-ink-muted hover:bg-surface-overlay"
                }`}
              >
                {batchMode && (
                  <input
                    type="checkbox"
                    checked={selectedIds.has(conv.id)}
                    onChange={() => toggleSelect(conv.id)}
                    className="accent-ink-accent"
                  />
                )}
                <button
                  type="button"
                  onClick={() => {
                    if (!batchMode) {
                      selectSession(conv.id);
                      resumeMut.mutate(conv.id);
                    }
                  }}
                  disabled={batchMode}
                  aria-current={selectedSessionId === conv.id ? "true" : undefined}
                  title={conv.title}
                  className="flex min-w-0 flex-1 items-center gap-1.5 focus-visible:ring-2 focus-visible:ring-ink-accent disabled:cursor-default"
                >
                  <span aria-hidden="true" className="shrink-0 text-xs">{kindIcon(conv.kind)}</span>
                  <span className="min-w-0 flex-1 truncate">{conv.title}</span>
                  <WorkspaceBadge
                    workspaceId={conv.workspaceId}
                    workspaceRootPath={workspacePathMap.get(conv.workspaceId) ?? null}
                  />
                  <span className="shrink-0 text-xs tabular-nums">
                    {formatRelativeTime(conv.createdAt, Date.now(), locale)}
                  </span>
                </button>
                {!batchMode && (
                  <button
                    type="button"
                    onClick={() => setConfirm({ type: "delete", sessionId: conv.id, title: conv.title })}
                    className="shrink-0 text-xs text-ink-muted opacity-0 transition-opacity hover:text-red-500 group-hover:opacity-100"
                    aria-label={t("conversation.delete")}
                  >
                    🗑
                  </button>
                )}
              </div>
            </li>
          ))}
        </ul>
      </AsyncBoundary>
      </div>

      {!batchMode && <div className="shrink-0 border-t border-ink-muted/30 pt-1 mt-1"><WorkspaceFilter /></div>}

      {dialogKind && (
        <NewConversationDialog
          kind={dialogKind}
          onClose={() => setDialogKind(null)}
        />
      )}

      <Dialog
        open={confirm !== null}
        title={confirmTitle}
        onClose={() => setConfirm(null)}
        footer={
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={() => setConfirm(null)}
              className="rounded border border-ink-muted/40 px-3 py-1 text-sm text-ink-muted hover:bg-surface-overlay"
            >
              {t("conversation.cancel")}
            </button>
            <button
              type="button"
              onClick={() => void handleConfirm()}
              className="rounded bg-red-600 px-3 py-1 text-sm text-white hover:bg-red-700"
            >
              {t("conversation.confirm")}
            </button>
          </div>
        }
      >
        <p className="py-2 text-sm text-ink">{confirmMessage}</p>
      </Dialog>
    </section>
  );
}
