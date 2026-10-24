import type { ReactNode } from "react";
import { toast } from "../../lib/store/toastStore";

export interface BubbleToolbarAction {
  key: string;
  label: string;
  icon: ReactNode;
  onClick: () => void;
}

interface BubbleToolbarProps {
  actions: BubbleToolbarAction[];
  /** Which edge of the bubble the bar hugs (assistant/group: left, user: right). */
  align?: "left" | "right";
}

/** WeChat-style action bar that pops above a bubble on click: quote /
 *  rollback / copy in single chat, quote / copy in group chat. Rendered
 *  inside the (position:relative) bubble body at bottom-full. */
export function BubbleToolbar({ actions, align = "left" }: BubbleToolbarProps): ReactNode {
  if (actions.length === 0) return null;
  return (
    <div
      data-testid="bubble-toolbar"
      className={`absolute bottom-full z-20 mb-1 flex w-max items-center gap-0.5 whitespace-nowrap rounded-md border border-ink-muted/40 bg-surface-raised px-1 py-0.5 shadow-md ${
        align === "right" ? "right-0" : "left-0"
      }`}
    >
      {actions.map((action) => (
        <button
          key={action.key}
          type="button"
          onClick={action.onClick}
          className="flex h-7 shrink-0 items-center gap-1 whitespace-nowrap rounded px-2 text-xs text-ink-muted transition-colors hover:bg-surface-overlay hover:text-ink"
        >
          {action.icon}
          <span>{action.label}</span>
        </button>
      ))}
    </div>
  );
}

/** Toolbar icon set — stroke (outline) style, 1.5px line width: matches
 *  the QuotePreviewBar close icon and stays legible at 14px render size.
 *  Quote = paired quotation marks (WeChat semantics), rollback =
 *  counter-clockwise arrow (universal undo), copy = offset rects. */
export const QUOTE_ICON = (
  <svg viewBox="0 0 16 16" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <path d="M6.5 5v3.5a2.5 2.5 0 0 1-2.5 2.5" />
    <path d="M12 5v3.5a2.5 2.5 0 0 1-2.5 2.5" />
  </svg>
);

export const ROLLBACK_ICON = (
  <svg viewBox="0 0 16 16" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <path d="M3 3v3.5h3.5" />
    <path d="M3.4 6.5A4.75 4.75 0 1 0 7.5 3.2" />
  </svg>
);

const COPY_ICON = (
  <svg viewBox="0 0 16 16" className="h-3.5 w-3.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
    <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" />
    <path d="M10.5 3.5h-6a2 2 0 0 0-2 2v6" />
  </svg>
);

/** Copy action shared by single-chat and group bubbles. Toasts the
 *  outcome; returns null when there is no text to copy. `onDone` closes
 *  the toolbar (the action consumed the click). */
export function makeCopyAction(
  text: string | undefined,
  t: (key: string) => string,
  onDone: () => void,
): BubbleToolbarAction | null {
  if (text === undefined || text === "") return null;
  return {
    key: "copy",
    label: t("chat.copy"),
    icon: COPY_ICON,
    onClick: () => {
      onDone();
      void navigator.clipboard?.writeText(text).then(
        () => toast.success(t("chat.copied")),
        () => toast.error(t("chat.copyFailed")),
      );
    },
  };
}
