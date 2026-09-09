import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useDroppable } from "@dnd-kit/core";
import type { TaskDto } from "../../lib/ipc/bindings.gen";
import { TaskCard } from "./TaskCard";
import { Icon } from "../../components/ui/Icon/Icon";
import { statusLabelKey, type TaskStatus } from "./taskStatuses";

interface BoardColumnProps {
  status: TaskStatus;
  tasks: TaskDto[];
  onOpenRuns: (taskId: string) => void;
  onMove: (taskId: string, status: TaskStatus) => void;
  onDelete: (taskId: string) => void;
  /** 批次二② run-all — only the queued column receives this. */
  onRunAll?: () => void;
  runAllPending?: boolean;
  onRunWithTeam: (taskId: string, teamId: string) => void;
  onAutoFormRun: (taskId: string) => void;
}

/** One kanban column = one droppable status bucket; the queued column header
 * carries a two-step "run all" dispatch button (keyboard friendly, disabled
 * at N=0). */
export function BoardColumn({
  status,
  tasks,
  onOpenRuns,
  onMove,
  onDelete,
  onRunAll,
  runAllPending,
  onRunWithTeam,
  onAutoFormRun,
}: BoardColumnProps): ReactNode {
  const { t } = useTranslation();
  const [confirmRunAll, setConfirmRunAll] = useState(false);
  const { setNodeRef, isOver } = useDroppable({ id: status });
  return (
    <section
      ref={setNodeRef}
      aria-label={t(statusLabelKey(status))}
      className={`sketch-card flex min-h-0 flex-col ${
        isOver ? "border-ink-accent bg-surface-raised" : "bg-surface"
      }`}
    >
      <div className="flex shrink-0 items-center justify-between gap-1 px-2 py-1.5">
        <h3 className="text-title-hand text-xs font-semibold uppercase tracking-wide text-ink-muted">
          {t(statusLabelKey(status))}
        </h3>
        <span className="flex items-center gap-1">
          {onRunAll !== undefined &&
            (confirmRunAll ? (
              <>
                <button
                  type="button"
                  onClick={() => {
                    setConfirmRunAll(false);
                    onRunAll();
                  }}
                  disabled={runAllPending === true}
                  aria-label={t("board.runAllConfirm", { count: tasks.length })}
                  className="rounded bg-state-danger px-1.5 py-0.5 text-[10px] text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
                >
                  {t("board.runAllConfirm", { count: tasks.length })}
                </button>
                <button
                  type="button"
                  onClick={() => setConfirmRunAll(false)}
                  disabled={runAllPending === true}
                  aria-label={t("common.cancel")}
                  className="rounded px-1 py-0.5 text-[10px] text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
                >
                  <Icon name="close" size={12} />
                </button>
              </>
            ) : (
              <button
                type="button"
                onClick={() => setConfirmRunAll(true)}
                disabled={tasks.length === 0 || runAllPending === true}
                aria-label={t("board.runAll")}
                className="rounded border border-ink-muted/60 px-1.5 py-0.5 text-[10px] text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
              >
                {t("board.runAll")}
              </button>
            ))}
          <span className="rounded bg-surface-overlay px-1.5 text-ink-muted">{tasks.length}</span>
        </span>
      </div>
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-2">
        {tasks.map((task) => (
          <TaskCard
            key={task.id}
            task={task}
            onOpenRuns={onOpenRuns}
            onMove={onMove}
            onDelete={onDelete}
            onRunWithTeam={onRunWithTeam}
            onAutoFormRun={onAutoFormRun}
          />
        ))}
        {tasks.length === 0 && <p className="px-1 text-xs text-ink-muted">—</p>}
      </div>
    </section>
  );
}
