import type { ReactNode } from "react";

export interface RoundIndicatorProps {
  current: number;
  max: number;
  speaker?: string;
}

/** Round progress indicator for group conversations. */
export function RoundIndicator({ current, max, speaker }: RoundIndicatorProps): ReactNode {
  const pct = max > 0 ? Math.min(100, (current / max) * 100) : 0;
  return (
    <div className="flex items-center gap-2 px-3 py-1 text-xs text-ink-muted">
      <span>Round {current} / {max}</span>
      {speaker && <span className="rounded bg-ink-muted/20 px-1">{speaker}</span>}
      <div className="h-1 w-20 overflow-hidden rounded bg-ink-muted/20">
        <div className="h-full bg-ink-accent" style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}
