import type { ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ipc } from "../../lib/ipc/client";
import { useUiStore } from "../../lib/store/uiStore";

/**
 * Workspace filter dropdown for the conversations list. Renders "全部" plus
 * each registered workspace. Selection updates `uiStore.conversationWorkspaceFilter`
 * which drives the `listConversations` query.
 */
export function WorkspaceFilter(): ReactNode {
  const { t } = useTranslation();
  const filter = useUiStore((s) => s.conversationWorkspaceFilter);
  const setFilter = useUiStore((s) => s.setConversationWorkspaceFilter);

  const workspacesQuery = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => ipc.listWorkspaces(),
    staleTime: 30_000,
  });

  const workspaces = workspacesQuery.data ?? [];

  return (
    <div className="flex items-center gap-1">
      <label className="text-xs text-ink-muted">{t("conversation.workspaceFilterLabel")}:</label>
      <select
        value={filter}
        onChange={(e) => setFilter(e.target.value)}
        className="rounded border border-ink-muted/40 bg-surface-raised px-1.5 py-0.5 text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
      >
        <option value="all">{t("conversation.workspaceFilterAll")}</option>
        {workspaces.map((ws) => (
          <option key={ws.id} value={ws.id}>
            {ws.rootPath}
          </option>
        ))}
      </select>
    </div>
  );
}
