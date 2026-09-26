import type { ReactNode } from "react";

export interface ChatTabProps {
  sessionId: string;
  kind: string;
  title: string;
  active: boolean;
  onActivate: (sessionId: string) => void;
  onClose: (sessionId: string) => void;
}

/** Kind icon mapping (mirrors ConversationHeader.kindIcon). */
function kindIcon(kind: string): string {
  switch (kind) {
    case "chat": return "💬";
    case "group": return "👥";
    case "background": return "⚙";
    case "scheduled": return "⏰";
    default: return "💬";
  }
}

/** Truncate title to max chars, appending ellipsis if exceeded. */
function truncateTitle(title: string, max = 20): string {
  return title.length > max ? title.slice(0, max) + "…" : title;
}

/**
 * Single chat tab. Title is shown inline (max 20 chars, full title on hover
 * via title attribute). Active tab uses bg-surface + a surface-colored bottom
 * border to visually merge with the conversation header below (no separating
 * line under the active tab); inactive tabs keep the container's border-b.
 */
export function ChatTab({ sessionId, kind, title, active, onActivate, onClose }: ChatTabProps): ReactNode {
  const displayTitle = truncateTitle(title, 20);
  return (
    <button
      type="button"
      onClick={() => onActivate(sessionId)}
      title={title}
      className={`group flex shrink-0 items-center gap-1 px-2 py-1 text-xs transition-colors ${
        active
          ? "bg-surface text-ink-accent border-b-2 border-b-surface -mb-px"
          : "text-ink-muted hover:bg-surface-overlay hover:text-ink"
      }`}
      aria-selected={active}
      role="tab"
    >
      <span aria-hidden="true" className="text-xs">{kindIcon(kind)}</span>
      <span className="truncate">{displayTitle}</span>
      <span
        role="button"
        aria-label="close"
        onClick={(e) => {
          e.stopPropagation();
          onClose(sessionId);
        }}
        className="ml-0.5 text-xs text-ink-muted opacity-0 transition-opacity hover:text-state-danger group-hover:opacity-100"
      >
        ×
      </span>
    </button>
  );
}
