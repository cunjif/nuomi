import type { ReactNode } from "react";

export interface ChatTabProps {
  sessionId: string;
  kind: string;
  title: string;
  active: boolean;
  onActivate: (sessionId: string) => void;
  onClose: (sessionId: string) => void;
}

/** Kind icon mapping for chat tabs (chat / group / background / scheduled). */
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
 * via title attribute). The tab is a 40px-tall rounded chip bottom-aligned in
 * the strip row. Active tab = highlighted chip (lighter background, no
 * border); inactive tabs stay transparent and only reveal a hover background.
 */
export function ChatTab({ sessionId, kind, title, active, onActivate, onClose }: ChatTabProps): ReactNode {
  const displayTitle = truncateTitle(title, 20);
  return (
    <button
      type="button"
      onClick={() => onActivate(sessionId)}
      title={title}
      className={`group flex h-10 shrink-0 items-center gap-2 rounded-md px-2.5 text-xs transition-colors ${
        active
          ? "bg-surface-overlay text-ink-accent"
          : "bg-transparent text-ink-muted hover:bg-surface-overlay hover:text-ink"
      }`}
      aria-selected={active}
      role="tab"
    >
      <span aria-hidden="true" className="text-sm leading-none">{kindIcon(kind)}</span>
      <span className="truncate">{displayTitle}</span>
      <span
        role="button"
        aria-label="close"
        onClick={(e) => {
          e.stopPropagation();
          onClose(sessionId);
        }}
        className="-mr-1 ml-0.5 flex size-5 items-center justify-center text-xs leading-none text-ink-muted opacity-0 transition-opacity hover:text-state-danger group-hover:opacity-100 focus-visible:opacity-100"
      >
        ×
      </span>
    </button>
  );
}
