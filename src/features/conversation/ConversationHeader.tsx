import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ConversationDto } from "../../lib/ipc/client";
import { Icon } from "../../components/ui/Icon/Icon";

export interface ConversationHeaderProps {
  conversation: ConversationDto | null;
  /** Optional right-side status node (group: Round, background: RunState, scheduled: next trigger). */
  statusSlot?: ReactNode;
  /** Called when the user clicks the Agent sidebar entry button. */
  onAgentSidebarOpen?: () => void;
}

/** Kind icon + label mapping. */
function kindIcon(kind: string): { icon: string; label: string } {
  switch (kind) {
    case "chat":
      return { icon: "💬", label: "Chat" };
    case "group":
      return { icon: "👥", label: "Group" };
    case "background":
      return { icon: "⚙", label: "Background" };
    case "scheduled":
      return { icon: "⏰", label: "Scheduled" };
    default:
      return { icon: "💬", label: kind };
  }
}

/**
 * Unified header for all conversation kinds. Shows the kind icon, title,
 * agent/team badge, and an optional type-specific status slot.
 */
export function ConversationHeader({ conversation, statusSlot, onAgentSidebarOpen }: ConversationHeaderProps): ReactNode {
  const { t } = useTranslation();

  if (!conversation) {
    return (
      <header className="flex shrink-0 items-center gap-2 border-b border-ink-muted/30 px-3 py-2">
        <span className="text-sm text-ink-muted">{t("chat.noSession")}</span>
      </header>
    );
  }

  const { icon, label } = kindIcon(conversation.kind);

  return (
    <header className="flex shrink-0 items-center gap-2 border-b border-ink-muted/30 px-3 py-2">
      <span aria-hidden="true" title={label}>{icon}</span>
      <h1 className="min-w-0 flex-1 truncate text-sm font-medium text-ink">{conversation.title}</h1>
      {conversation.teamId && (
        <span className="rounded bg-ink-muted/20 px-1.5 py-0.5 text-xs text-ink-muted">
          {t("conversation.teamBadge")}
        </span>
      )}
      {statusSlot}
      {onAgentSidebarOpen && (
        <button
          type="button"
          onClick={onAgentSidebarOpen}
          className="flex items-center justify-center rounded p-1 text-ink-muted hover:bg-surface-overlay hover:text-ink-accent"
          aria-label={t("conversation.agentSidebar.open")}
          title={t("conversation.agentSidebar.open")}
        >
          <Icon name="users" size={16} />
        </button>
      )}
    </header>
  );
}
