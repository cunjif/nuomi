import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Dialog } from "../../components/ui/Dialog";
import { ipc } from "../../lib/ipc/client";
import { describeError } from "../../i18n";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";
import { formatRelativeTime, type RelativeTimeLocale } from "../../lib/format/relativeTime";

function kindIcon(kind: string): string {
  switch (kind) {
    case "chat": return "💬";
    case "group": return "👥";
    case "background": return "⚙";
    case "scheduled": return "⏰";
    default: return "💬";
  }
}

interface ArchivedConversationsDialogProps {
  open: boolean;
  onClose: () => void;
}

export function ArchivedConversationsDialog({ open, onClose }: ArchivedConversationsDialogProps): ReactNode {
  const { t, i18n } = useTranslation();
  const qc = useQueryClient();
  const selectSession = useUiStore((s) => s.selectSession);
  const locale: RelativeTimeLocale = i18n.language === "en" ? "en" : "zh-CN";
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);

  const archivedQuery = useQuery({
    queryKey: ["archivedConversations"],
    queryFn: () => ipc.listArchivedConversations(),
    enabled: open,
    staleTime: 10_000,
  });

  const workspacesQuery = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => ipc.listWorkspaces(),
    staleTime: 30_000,
  });

  const restoreMut = useMutation({
    mutationFn: (sessionId: string) => ipc.restoreArchivedConversation(sessionId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["archivedConversations"] });
      void qc.invalidateQueries({ queryKey: ["conversations"] });
      toast.success(t("conversation.archivedRestored"));
    },
    onError: (e) => toast.error(`${t("conversation.archivedRestoreFailed")}: ${describeError(e)}`),
  });

  const deleteMut = useMutation({
    mutationFn: (sessionId: string) => ipc.deleteArchivedConversation(sessionId),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["archivedConversations"] });
      void qc.invalidateQueries({ queryKey: ["conversations"] });
      toast.success(t("conversation.archivedDeleted"));
    },
    onError: (e) => toast.error(`${t("conversation.archivedDeleteFailed")}: ${describeError(e)}`),
  });

  const conversations = archivedQuery.data ?? [];
  const workspaces = workspacesQuery.data ?? [];

  const isRestorable = (archivedWorkspacePath: string | null | undefined): boolean =>
    archivedWorkspacePath != null &&
    workspaces.some((ws) => ws.rootPath === archivedWorkspacePath);

  const handleView = (sessionId: string): void => {
    selectSession(sessionId);
    onClose();
  };

  const handleRestore = (sessionId: string): void => {
    void restoreMut.mutateAsync(sessionId);
  };

  const handleDelete = (sessionId: string): void => {
    setPendingDelete(null);
    void deleteMut.mutateAsync(sessionId);
  };

  return (
    <Dialog
      open={open}
      title={t("conversation.archivedTitle")}
      onClose={onClose}
      className="max-w-2xl"
    >
      <div className="max-h-[60vh] overflow-y-auto">
        {archivedQuery.isLoading ? (
          <p className="py-4 text-center text-sm text-ink-muted">{t("conversation.archivedLoading")}</p>
        ) : conversations.length === 0 ? (
          <p className="py-4 text-center text-sm text-ink-muted">{t("conversation.archivedEmpty")}</p>
        ) : (
          <ul className="flex flex-col gap-0.5">
            {conversations.map((conv) => (
              <li key={conv.id}>
                <div className="group flex w-full items-center gap-1.5 rounded px-2 py-1 text-sm">
                  <span aria-hidden="true" className="shrink-0 text-xs">{kindIcon(conv.kind)}</span>
                  <button
                    type="button"
                    onClick={() => handleView(conv.id)}
                    title={conv.title}
                    className="flex min-w-0 flex-1 flex-col items-start text-left focus-visible:ring-2 focus-visible:ring-ink-accent"
                  >
                    <span className="min-w-0 w-full truncate text-ink">{conv.title}</span>
                    <span className="min-w-0 w-full truncate text-xs text-ink-muted">
                      {conv.archivedWorkspacePath ?? t("conversation.archivedUnknownWorkspace")}
                      {conv.archivedDirectoryPath ? ` / ${conv.archivedDirectoryPath}` : ""}
                    </span>
                  </button>
                  <span className="shrink-0 text-xs tabular-nums text-ink-muted">
                    {conv.archivedAt != null ? formatRelativeTime(conv.archivedAt, Date.now(), locale) : ""}
                  </span>
                  {pendingDelete === conv.id ? (
                    <div className="flex shrink-0 gap-1">
                      <button
                        type="button"
                        onClick={() => handleDelete(conv.id)}
                        disabled={deleteMut.isPending}
                        className="rounded bg-state-danger px-1.5 py-0.5 text-xs text-surface disabled:opacity-30"
                      >
                        {t("conversation.confirm")}
                      </button>
                      <button
                        type="button"
                        onClick={() => setPendingDelete(null)}
                        className="rounded border border-ink-muted/40 px-1.5 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay"
                      >
                        {t("conversation.cancel")}
                      </button>
                    </div>
                  ) : (
                    <div className="flex shrink-0 gap-1">
                      {isRestorable(conv.archivedWorkspacePath) && (
                        <button
                          type="button"
                          onClick={() => handleRestore(conv.id)}
                          disabled={restoreMut.isPending}
                          className="rounded border border-ink-muted/40 px-1.5 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay disabled:opacity-30"
                          title={t("conversation.archivedRestore")}
                        >
                          ↩
                        </button>
                      )}
                      <button
                        type="button"
                        onClick={() => setPendingDelete(conv.id)}
                        className="text-xs text-ink-muted opacity-0 transition-opacity hover:text-state-danger group-hover:opacity-100"
                        aria-label={t("conversation.delete")}
                      >
                        🗑
                      </button>
                    </div>
                  )}
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
    </Dialog>
  );
}
