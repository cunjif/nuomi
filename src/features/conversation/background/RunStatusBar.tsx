import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { RunDto } from "../../../lib/ipc/client";

export interface RunStatusBarProps {
  run: RunDto | null;
  onStop: () => void;
}

/** Status badge + elapsed time + stop/retry for background runs. */
export function RunStatusBar({ run, onStop }: RunStatusBarProps): ReactNode {
  const { t } = useTranslation();
  if (!run) return null;
  const statusColors: Record<string, string> = {
    queued: "text-ink-muted",
    running: "text-ink-accent animate-pulse",
    awaiting_approval: "text-state-warning",
    succeeded: "text-state-ok",
    failed: "text-state-danger",
    timed_out: "text-state-danger",
    cancelled: "text-ink-muted",
    interrupted: "text-state-warning",
  };
  return (
    <div className="flex items-center gap-2 border-b border-ink-muted/30 px-3 py-1.5 text-xs">
      <span className={`font-medium ${statusColors[run.status] ?? "text-ink"}`}>{run.status}</span>
      <span className="text-ink-muted">{t("conversation.runId")}: {run.id.slice(0, 8)}</span>
      {run.cancelable && (
        <button
          type="button"
          onClick={onStop}
          className="ml-auto rounded border border-ink-muted/40 px-1.5 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay"
        >
          {t("conversation.stop")}
        </button>
      )}
    </div>
  );
}
