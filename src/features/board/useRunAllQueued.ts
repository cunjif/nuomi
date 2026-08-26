import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

type QueryClient = ReturnType<typeof useQueryClient>;

/** 批次二② run-all hook: dispatch every queued task one by one (strictly
 * sequential awaits); a single failure never stops the rest, and the summary
 * toast reports both the dispatched count and the failures. */
export function useRunAllQueued(qc: QueryClient): {
  runAllPending: boolean;
  runAllQueued: (queuedIds: string[]) => void;
} {
  const { t } = useTranslation();
  const [runAllPending, setRunAllPending] = useState(false);

  const runAllQueued = (queuedIds: string[]): void => {
    if (queuedIds.length === 0 || runAllPending) return;
    setRunAllPending(true);
    void (async () => {
      let dispatched = 0;
      for (const taskId of queuedIds) {
        try {
          await ipc.updateTaskStatus(taskId, "running");
          dispatched += 1;
          void qc.invalidateQueries({ queryKey: ["runs", taskId] });
        } catch {
          // counted in the summary toast below
        }
      }
      setRunAllPending(false);
      void qc.invalidateQueries({ queryKey: ["tasks"] });
      const total = queuedIds.length;
      const failed = total - dispatched;
      if (failed === 0) {
        toast.success(t("board.runAllStarted", { ok: dispatched, total }));
      } else {
        toast.error(t("board.runAllFailed", { failed, ok: dispatched, total }));
      }
    })();
  };

  return { runAllPending, runAllQueued };
}
