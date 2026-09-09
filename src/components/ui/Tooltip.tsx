import type { ReactNode } from "react";

/**
 * Lightweight hand-drawn tooltip (review §6.2). Wraps any trigger; the bubble
 * appears on hover/focus. Falls back to the native `title` attribute so the
 * hint is never lost when the bubble is clipped by overflow.
 */
export interface TooltipProps {
  label: ReactNode;
  children: ReactNode;
  className?: string;
}

export function Tooltip({ label, children, className = "" }: TooltipProps): ReactNode {
  return (
    <span className={`group relative inline-flex ${className}`} title={typeof label === "string" ? label : undefined}>
      {children}
      <span
        role="tooltip"
        className="pointer-events-none absolute bottom-full left-1/2 z-50 mb-1.5 -translate-x-1/2 whitespace-nowrap rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-dashed border-ink-muted bg-surface-raised px-2 py-1 font-note-hand text-xs text-ink opacity-0 shadow-sketch-sm transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100"
      >
        {label}
      </span>
    </span>
  );
}
