import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { describeError } from "../../i18n";

/**
 * Global background task tray. Shows all active runs with stop buttons.
 * Rendered in the shell header area for global visibility.
 */
export function BackgroundTray(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const runsQuery = useQuery({
    queryKey: ["activeRuns"],
    queryFn: () => ipc.listActiveRuns(),
    refetchInterval: 5_000,
  });

  const runs = runsQuery.data ?? [];
  if (runs.length === 0) return null;

  const handleStop = async (runId: string): Promise<void> => {
    try {
      await ipc.cancelRun(runId);
      void qc.invalidateQueries({ queryKey: ["activeRuns"] });
    } catch (e) {
      toast.error(describeError(e));
    }
  };

  return (
    <div className="flex items-center gap-1 border-b border-ink-muted/30 bg-surface-raised px-2 py-1 text-xs">
      <span className="text-ink-muted">{t("conversation.activeRuns")}:</span>
      {runs.map((run) => (
        <button
          key={run.id}
          type="button"
          onClick={() => void handleStop(run.id)}
          className="flex items-center gap-1 rounded bg-surface-overlay px-1.5 py-0.5 text-ink hover:bg-ink-muted/20"
        >
          <span className="animate-pulse">●</span>
          <span>{run.sessionId.slice(0, 8)}</span>
          <span className="text-ink-muted">{run.status}</span>
          <span aria-hidden="true">✕</span>
        </button>
      ))}
    </div>
  );
}
