import type { ReactNode } from "react";

/** Small inline spinner (theme-token colors only). */
export function Spinner({ label }: { label?: ReactNode }): ReactNode {
  return (
    <span className="inline-flex items-center gap-2" role="status">
      <span
        aria-hidden="true"
        className="inline-block size-4 animate-spin rounded-full border-2 border-ink-muted border-t-transparent"
      />
      {label !== undefined && <span className="text-xs">{label}</span>}
    </span>
  );
}
