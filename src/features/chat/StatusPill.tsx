import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../../components/ui/Icon/Icon";

/** One line of the pill's status history. `text` is already translated by
 *  the producer (8.2/8.3 wiring); `at` is the epoch-ms timestamp the entry
 *  became current (also used as the React list key). */
export interface StatusPillEntry {
  text: string;
  at: number;
}

export interface StatusPillProps {
  /** Chronological status history; the LAST entry is the current status. */
  entries: StatusPillEntry[];
  /** Epoch-ms wait start — the elapsed timer counts up from 0s. */
  startedAt: number;
  /** Fade-out phase: the pill scales down and fades (200ms, transform/opacity
   *  only); the parent unmounts it after the transition. */
  closing?: boolean;
  /** Optional stop affordance (spec 5.8.1-5). Rendered only when provided. */
  onStop?: () => void;
}

const TIMER_INTERVAL_MS = 1000;
const DOT_STAGGER_MS = 200;

/**
 * Compact in-flow status pill (spec 5.8.1-1): three-dot loading animation +
 * current status copy + elapsed timer + expandable history. All motion is
 * CSS transform/opacity only — `animate-draw-in` (200ms) covers the pill
 * entrance and each status-copy swap, `animate-pulse` covers the staggered
 * dots (and is disabled under prefers-reduced-motion), and the chevron
 * rotation plus closing fade ride Tailwind transitions.
 */
export function StatusPill({
  entries,
  startedAt,
  closing = false,
  onStop,
}: StatusPillProps): ReactNode {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), TIMER_INTERVAL_MS);
    return () => clearInterval(timer);
  }, []);

  if (entries.length === 0) return null;

  const current = entries[entries.length - 1]!;
  const elapsedSeconds = Math.max(0, Math.floor((now - startedAt) / 1000));

  return (
    <div
      data-testid="status-pill"
      className={`sketch-card inline-flex animate-draw-in flex-col items-start gap-1 bg-surface-raised px-3 py-1.5 shadow-sketch-sm transition-[opacity,transform] duration-200 ease-out ${
        closing ? "scale-95 opacity-0" : "scale-100 opacity-100"
      }`}
    >
      <div className="flex max-w-full items-center gap-2 text-sm">
        <span className="flex shrink-0 items-center gap-0.5" aria-hidden="true">
          {[0, 1, 2].map((i) => (
            <span
              key={i}
              className="h-1 w-1 animate-pulse rounded-full bg-ink-muted"
              style={{ animationDelay: `${i * DOT_STAGGER_MS}ms` }}
            />
          ))}
        </span>
        <span key={current.text} role="status" className="animate-draw-in truncate text-ink">
          {current.text}
        </span>
        <span
          className="shrink-0 font-mono text-xs text-ink-muted"
          aria-label={t("chat.status.pill.waiting", { seconds: elapsedSeconds })}
        >
          {elapsedSeconds}s
        </span>
        {onStop !== undefined && (
          <button
            type="button"
            aria-label={t("chat.status.pill.stop")}
            className="shrink-0 rounded p-0.5 text-ink-muted hover:bg-surface-overlay hover:text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
            onClick={onStop}
          >
            <Icon name="stop" size={14} />
          </button>
        )}
        <button
          type="button"
          aria-expanded={expanded}
          aria-label={expanded ? t("chat.status.pill.collapse") : t("chat.status.pill.expand")}
          className="shrink-0 rounded p-0.5 text-ink-muted hover:bg-surface-overlay hover:text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
          onClick={() => setExpanded((e) => !e)}
        >
          <span
            className={`block transition-transform duration-200 ease-out ${expanded ? "rotate-180" : ""}`}
          >
            <Icon name="chevron-down" size={14} />
          </span>
        </button>
      </div>

      {expanded && (
        <ul className="w-full animate-draw-in space-y-0.5 border-t border-divider pt-1 text-xs">
          {entries.map((entry, i) => {
            const isCurrent = i === entries.length - 1;
            return (
              <li
                key={`${entry.at}-${i}`}
                className={`flex items-center gap-1 ${isCurrent ? "text-ink" : "text-ink-muted"}`}
              >
                {isCurrent && (
                  <span className="shrink-0 rounded bg-surface-overlay px-1 text-[10px] text-ink-accent">
                    {t("chat.status.pill.current")}
                  </span>
                )}
                <span className="truncate">{entry.text}</span>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
