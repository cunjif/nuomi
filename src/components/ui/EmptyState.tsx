import type { ReactNode } from "react";
import { Icon, type IconName } from "./Icon/Icon";

/**
 * Hand-drawn empty placeholder (review §6.2 / §11.1). A dashed card framing a
 * glyph + title + hint, used wherever a list or pane has nothing to show.
 */
export interface EmptyStateProps {
  icon?: IconName;
  title: ReactNode;
  hint?: ReactNode;
  className?: string;
  action?: ReactNode;
}

export function EmptyState({ icon = "note", title, hint, className = "", action }: EmptyStateProps): ReactNode {
  return (
    <div
      className={`flex flex-col items-center justify-center gap-2 rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-dashed border-ink-muted/60 bg-surface-raised/40 px-4 py-8 text-center ${className}`}
    >
      <Icon name={icon} size={28} className="text-ink-muted" />
      <p className="font-note-hand text-sm text-ink">{title}</p>
      {hint !== undefined && <p className="max-w-xs text-xs text-ink-muted">{hint}</p>}
      {action}
    </div>
  );
}
