import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useDroppable } from "@dnd-kit/core";
import type { TaskDto } from "../../lib/ipc/bindings.gen";
import { TaskCard } from "./TaskCard";
import { statusLabelKey, type TaskStatus } from "./taskStatuses";

interface BoardColumnProps {
  status: TaskStatus;
  tasks: TaskDto[];
  onOpenRuns: (taskId: string) => void;
  onMove: (taskId: string, status: TaskStatus) => void;
  onRunWithTeam: (taskId: string, teamId: string) => void;
  onAutoFormRun: (taskId: string) => void;
}

/** One kanban column = one droppable status bucket. */
export function BoardColumn({ status, tasks, onOpenRuns, onMove, onRunWithTeam, onAutoFormRun }: BoardColumnProps): ReactNode {
  const { t } = useTranslation();
  const { setNodeRef, isOver } = useDroppable({ id: status });
  return (
    <section
      ref={setNodeRef}
      aria-label={t(statusLabelKey(status))}
      className={`flex min-h-0 flex-col rounded border ${
        isOver ? "border-ink-accent bg-surface-raised" : "border-ink-muted/30 bg-surface"
      }`}
    >
      <h3 className="flex shrink-0 items-center justify-between px-2 py-1.5 text-xs font-semibold uppercase tracking-wide text-ink-muted">
        <span>{t(statusLabelKey(status))}</span>
        <span className="rounded bg-surface-overlay px-1.5 text-ink-muted">{tasks.length}</span>
      </h3>
      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-2">
        {tasks.map((task) => (
          <TaskCard
            key={task.id}
            task={task}
            onOpenRuns={onOpenRuns}
            onMove={onMove}
            onRunWithTeam={onRunWithTeam}
            onAutoFormRun={onAutoFormRun}
          />
        ))}
        {tasks.length === 0 && <p className="px-1 text-xs text-ink-muted">—</p>}
      </div>
    </section>
  );
}
