import type { ReactNode } from "react";
import { useState, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ipc } from "../../lib/ipc/client";
import { useUiStore } from "../../lib/store/uiStore";
import { WorkspacePathBar } from "./WorkspacePathBar";
import { AgentSidebar } from "./sidebar/AgentSidebar";
import { ChatView } from "../chat/ChatView";
import { GroupConversationView } from "./group/GroupConversationView";
import { BackgroundConversationView } from "./background/BackgroundConversationView";
import { ScheduledConversationView } from "./scheduled/ScheduledConversationView";
import { isRoleReady } from "../../lib/conversation/roleReady";
import { WorkspaceListDialog } from "../shell/WorkspaceListDialog";

/**
 * Top-level conversation surface. Fetches the current conversation and
 * dispatches to a kind-specific view (chat / group / background / scheduled).
 */
export function ConversationView(): ReactNode {
  const { t } = useTranslation();
  const sessionId = useUiStore((s) => s.selectedSessionId);
  const setView = useUiStore((s) => s.setView);
  const activeWorkspaceId = useUiStore((s) => s.activeWorkspaceId);
  const [sidebarOpen, setSidebarOpen] = useState(false);
  const [wsListOpen, setWsListOpen] = useState(false);

  const rolesQuery = useQuery({
    queryKey: ["roles"],
    queryFn: () => ipc.listRoles(),
    staleTime: 30_000,
  });

  const convQuery = useQuery({
    queryKey: ["conversation", sessionId],
    queryFn: () => (sessionId ? ipc.getConversation(sessionId) : Promise.reject(new Error("no session"))),
    enabled: sessionId !== null,
    staleTime: 10_000,
  });

  const workspacesQuery = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => ipc.listWorkspaces(),
    staleTime: 10_000,
  });
  const workspaceRootPath = useMemo(() => {
    if (!activeWorkspaceId) return null;
    const ws = workspacesQuery.data?.find((w) => w.id === activeWorkspaceId);
    return ws?.rootPath ?? null;
  }, [activeWorkspaceId, workspacesQuery.data]);

  // No Role Agent configured → conversation panel is unavailable.
  const roleAgents = (rolesQuery.data ?? []).filter(isRoleReady);
  if (roleAgents.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 p-6">
        <p className="text-sm font-medium text-ink">{t("chat.noRoleAgent")}</p>
        <p className="max-w-xs text-center text-xs text-ink-muted">{t("chat.noRoleAgentHint")}</p>
        <button
          type="button"
          onClick={() => setView("settings")}
          className="pixel-fill-accent px-3 py-1.5 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent"
        >
          {t("chat.goToSettings")}
        </button>
      </div>
    );
  }

  if (sessionId === null) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-sm text-ink-muted">
        {t("chat.noSession")}
      </div>
    );
  }

  const kind = convQuery.data?.kind ?? "chat";

  return (
    <div className="flex h-full flex-col">
      <WorkspacePathBar
        rootPath={workspaceRootPath}
        conversation={convQuery.data ?? null}
        onOpenWorkspaceList={() => setWsListOpen(true)}
        onAgentSidebarOpen={() => setSidebarOpen(true)}
      />
      <div className="relative min-h-0 flex-1">
        {renderKindView(kind)}
        {convQuery.data && (
          <AgentSidebar
            conversation={convQuery.data}
            sessionId={sessionId}
            open={sidebarOpen}
            onClose={() => setSidebarOpen(false)}
          />
        )}
      </div>
      <WorkspaceListDialog open={wsListOpen} onClose={() => setWsListOpen(false)} />
    </div>
  );
}

function renderKindView(kind: string): ReactNode {
  switch (kind) {
    case "chat":
      return <ChatView />;
    case "group":
      return <GroupConversationView />;
    case "background":
      return <BackgroundConversationView />;
    case "scheduled":
      return <ScheduledConversationView />;
    default:
      return <ChatView />;
  }
}
