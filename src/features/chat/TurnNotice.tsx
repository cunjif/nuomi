import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

/**
 * Terminal outcome of a turn that did NOT complete normally (SPEC 5.8.1-5,
 * 5.8.3-1, 5.8.3-3): user-stopped, agent failure, or an unrecoverable
 * stream interruption. Rendered inline right where the status pill sat —
 * the stopped kind is a plain statement; failed/interrupted carry a retry
 * affordance that re-sends the last input.
 */
export type TurnNoticeKind = "stopped" | "failed" | "interrupted";

export interface TurnNoticeProps {
  kind: TurnNoticeKind;
  /** Error summary shown for the failed kind (SPEC 5.8.3-1c). */
  summary?: string;
  /** Retry affordance for failed/interrupted (SPEC 5.8.3-1c/3c). */
  onRetry?: () => void;
}

export function TurnNotice({ kind, summary, onRetry }: TurnNoticeProps): ReactNode {
  const { t } = useTranslation();
  const text =
    kind === "stopped"
      ? t("chat.status.stopped")
      : kind === "failed"
        ? t("chat.status.agentFailed", { summary: summary ?? "" })
        : t("chat.status.interrupted");
  return (
    <div
      data-testid="turn-notice"
      data-kind={kind}
      className="sketch-card inline-flex animate-draw-in items-center gap-2 bg-surface-raised px-3 py-1.5 text-sm shadow-sketch-sm"
    >
      <span className="truncate text-ink-muted" role="status">
        {text}
      </span>
      {kind !== "stopped" && onRetry !== undefined && (
        <button
          type="button"
          className="shrink-0 rounded border border-ink-muted px-2 py-0.5 text-xs text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
          onClick={onRetry}
        >
          {t("common.retry")}
        </button>
      )}
    </div>
  );
}
