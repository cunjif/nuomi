import type { ReactNode } from "react";

/**
 * Hand-drawn spinner (review §7.2). A dashed-ink arc that rotates like a pencil
 * stroke — replaces the flat `border-t-transparent` ring. `animate-spin` is
 * disabled under prefers-reduced-motion (global.css), so it freezes safely.
 */
export function Spinner({ label }: { label?: ReactNode }): ReactNode {
  return (
    <span className="inline-flex items-center gap-2" role="status">
      <svg
        aria-hidden="true"
        width="16"
        height="16"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth={2}
        strokeLinecap="round"
        className="size-4 animate-spin text-ink-accent"
      >
        <path d="M19 12a7 7 0 1 1-3.5-6.05" />
      </svg>
      {label !== undefined && <span className="text-xs">{label}</span>}
    </span>
  );
}
