import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ConversationDto } from "../../lib/ipc/client";
import { Icon } from "../../components/ui/Icon/Icon";
import { RoleAvatar } from "./composer/RoleAvatar";

export interface WorkspacePathBarProps {
  /** Full root path of the active workspace (shown verbatim, incl. any \\?\ prefix). */
  rootPath: string | null;
  /** Current conversation; null hides the avatar + more button. */
  conversation: ConversationDto | null;
  /** Opens the workspace list dialog (preserves the former subscript entry point). */
  onOpenWorkspaceList: () => void;
  /** Opens the Agent detail sidebar. */
  onAgentSidebarOpen: () => void;
}

/**
 * Panel header rendered directly below the chat tab strip, inside the tab
 * panel. Left: the active workspace's full root path (monospace; click opens
 * the workspace list). Right: the current conversation's Role avatar + a
 * "more" (···) button that opens the Agent detail sidebar. Fixed 48px row so
 * the header reads as a stable band regardless of path length. The row draws
 * no borders: the tab panel is borderless on top and the message stream below
 * provides no divider, so header and content read as one surface.
 */
export function WorkspacePathBar({
  rootPath,
  conversation,
  onOpenWorkspaceList,
  onAgentSidebarOpen,
}: WorkspacePathBarProps): ReactNode {
  const { t } = useTranslation();
  return (
    <div className="flex h-12 shrink-0 items-center gap-2 px-4">
      <button
        type="button"
        onClick={onOpenWorkspaceList}
        className="min-w-0 flex-1 truncate rounded px-1 py-0.5 text-left font-mono text-xs text-ink-muted hover:bg-surface-overlay hover:text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
        title={rootPath ?? ""}
      >
        {rootPath ?? ""}
      </button>
      {conversation && (
        <>
          <RoleAvatar conversation={conversation} size="md" />
          <button
            type="button"
            onClick={onAgentSidebarOpen}
            className="flex size-7 shrink-0 items-center justify-center rounded text-ink-muted hover:bg-surface-overlay hover:text-ink-accent focus-visible:ring-2 focus-visible:ring-ink-accent"
            aria-label={t("conversation.agentSidebar.open")}
            title={t("conversation.agentSidebar.open")}
          >
            <Icon name="more" size={16} />
          </button>
        </>
      )}
    </div>
  );
}
