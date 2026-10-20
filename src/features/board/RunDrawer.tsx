import type { ReactNode } from "react";
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { Icon } from "../../components/ui/Icon/Icon";
import { describeError } from "../../i18n";
import type { TaskDto } from "../../lib/ipc/bindings.gen";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";

const runStatusLabelKey = (status: string): string => `board.run_status_${status}`;

interface RunDrawerProps {
  /** Board tasks — supplies the human title for the drawer header (the
   * drawer itself only receives a task id through the ui store). */
  tasks: TaskDto[];
}

/** Run detail drawer: all runs for a task, refreshed by domain events.
 * Escape closes and focus lands in the panel (Dialog pattern); live runs
 * expose the cancel action that the `cancelable` DTO flag promises. */
export function RunDrawer({ tasks }: RunDrawerProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const taskId = useUiStore((s) => s.runDrawerTaskId);
  const setRunDrawerTask = useUiStore((s) => s.setRunDrawerTask);
  const panelRef = useRef<HTMLElement>(null);
  const runsQuery = useQuery({
    queryKey: ["runs", taskId],
    queryFn: () => ipc.listRunsByTask(taskId ?? ""),
    enabled: taskId !== null,
  });

  const taskTitle = tasks.find((task) => task.id === taskId)?.title ?? taskId;

  useEffect(() => {
    if (taskId === null) return undefined;
    panelRef.current?.focus();
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") setRunDrawerTask(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [taskId, setRunDrawerTask]);

  const cancelMut = useMutation({
    mutationFn: (runId: string) => ipc.cancelRun(runId),
    onSuccess: (_data, runId) => {
      void qc.invalidateQueries({ queryKey: ["runs", taskId] });
      void qc.invalidateQueries({ queryKey: ["tasks"] });
      toast.success(t("board.cancelRunStarted", { runId }));
    },
    onError: (e) => toast.error(`${t("board.cancelRunFailed")}: ${describeError(e)}`),
  });

  if (taskId === null) return null;
  const close = (): void => setRunDrawerTask(null);
  return (
    <div className="fixed inset-0 z-40 flex justify-end bg-surface-scrim" onClick={close}>
      <aside
        ref={panelRef}
        tabIndex={-1}
        role="dialog"
        aria-label={t("board.runsFor", { title: taskTitle })}
        className="flex h-full w-96 flex-col border-l border-ink-muted/40 bg-surface-raised p-3 focus:outline-none"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-2 flex items-center justify-between gap-2">
          <h2 className="font-title-hand truncate text-sm font-semibold">
            {t("board.runsFor", { title: taskTitle })}
          </h2>
          <button
            type="button"
            onClick={close}
            aria-label={t("common.close")}
            className="rounded px-2 text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            <Icon name="close" size={16} />
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
              <li key={run.id} className="sketch-card bg-surface p-2 text-xs">
                <div className="flex items-center justify-between gap-2">
                  <p className="truncate font-mono text-ink-muted">{run.id}</p>
                  {run.kind === "team" && (
                    <span className="shrink-0 rounded bg-surface-overlay px-1.5 text-[10px] text-ink-muted">
                      {t("board.runKindTeam")}
                    </span>
                  )}
                </div>
                <div className="mt-1 flex items-center justify-between gap-2">
                  <p>
                    <span className="text-ink-muted">{t("board.runStatus")}: </span>
                    <span className={run.status === "running" ? "text-state-warn" : "text-ink"}>
                      {t(runStatusLabelKey(run.status), { defaultValue: run.status })}
                    </span>
                  </p>
                  {run.cancelable && (
                    <button
                      type="button"
                      onClick={() => cancelMut.mutate(run.id)}
                      disabled={cancelMut.isPending}
                      aria-label={`${t("board.cancelRun")} ${run.id}`}
                      className="shrink-0 rounded border border-ink-muted/60 px-1.5 py-0.5 text-[10px] text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
                    >
                      {t("board.cancelRun")}
                    </button>
                  )}
                </div>
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
