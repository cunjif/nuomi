import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { ipc } from "../../lib/ipc/client";
import { useUiStore } from "../../lib/store/uiStore";

/** Run detail drawer: all runs for a task, refreshed by domain events. */
export function RunDrawer(): ReactNode {
  const { t } = useTranslation();
  const taskId = useUiStore((s) => s.runDrawerTaskId);
  const setRunDrawerTask = useUiStore((s) => s.setRunDrawerTask);
  const runsQuery = useQuery({
    queryKey: ["runs", taskId],
    queryFn: () => ipc.listRunsByTask(taskId ?? ""),
    enabled: taskId !== null,
  });
  if (taskId === null) return null;
  const close = (): void => setRunDrawerTask(null);
  return (
    <div className="fixed inset-0 z-40 flex justify-end bg-surface-scrim" onClick={close}>
      <aside
        role="dialog"
        aria-label={t("board.runsFor", { title: taskId })}
        className="flex h-full w-96 flex-col border-l border-ink-muted/40 bg-surface-raised p-3"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-2 flex items-center justify-between">
          <h2 className="truncate text-sm font-semibold">{t("board.runsFor", { title: taskId })}</h2>
          <button
            type="button"
            onClick={close}
            aria-label={t("common.close")}
            className="rounded px-2 text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            ×
          </button>
        </div>
        <AsyncBoundary
          isLoading={runsQuery.isLoading}
          error={runsQuery.error}
          isEmpty={(runsQuery.data?.length ?? 0) === 0}
          emptyLabel={t("board.noRuns")}
          onRetry={() => void runsQuery.refetch()}
        >
          <ul className="flex flex-col gap-2 overflow-y-auto">
            {(runsQuery.data ?? []).map((run) => (
              <li key={run.id} className="rounded border border-ink-muted/40 bg-surface p-2 text-xs">
                <p className="font-mono text-ink-muted">{run.id}</p>
                <p className="mt-1">
                  <span className="text-ink-muted">{t("board.runStatus")}: </span>
                  <span className={run.status === "running" ? "text-state-warn" : "text-ink"}>{run.status}</span>
                </p>
                <p>
                  <span className="text-ink-muted">{t("board.heartbeat")}: </span>
                  <span className="font-mono">{new Date(run.heartbeatAt).toLocaleString()}</span>
                </p>
              </li>
            ))}
          </ul>
        </AsyncBoundary>
      </aside>
    </div>
  );
}
