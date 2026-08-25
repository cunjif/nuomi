import type { ReactNode } from "react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { DndContext, type DragEndEvent } from "@dnd-kit/core";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { DOMAIN_CHANNEL } from "../../lib/events/types";
import { useDomainEvents } from "../../lib/events/useDomainEvents";
import { describeError } from "../../i18n";
import type { TaskDto } from "../../lib/ipc/bindings.gen";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";
import { BoardColumn } from "./BoardColumn";
import { NewTaskForm } from "./NewTaskForm";
import { RunDrawer } from "./RunDrawer";
import { isTaskStatus, TASK_STATUSES, type TaskStatus } from "./taskStatuses";

/** U11 kanban: five status columns, drag or menu to move, run drawer. */
export function BoardView(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [formOpen, setFormOpen] = useState(false);
  const setRunDrawerTask = useUiStore((s) => s.setRunDrawerTask);
  const tasksQuery = useQuery({ queryKey: ["tasks", null], queryFn: () => ipc.listTasks(null) });

  const moveMut = useMutation({
    mutationFn: ({ taskId, status }: { taskId: string; status: TaskStatus }) =>
      ipc.updateTaskStatus(taskId, status),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["tasks"] }),
    onError: (e) => toast.error(`${t("board.moveFailed")}: ${describeError(e)}`),
  });

  // Global channel keeps the board live (task/run state transitions).
  useDomainEvents([DOMAIN_CHANNEL], (batch) => {
    if (batch.some((e) => e.type.startsWith("task.") || e.type.startsWith("run."))) {
      void qc.invalidateQueries({ queryKey: ["tasks"] });
    }
  });

  const grouped = useMemo(() => {
    const map = new Map<TaskStatus, TaskDto[]>(TASK_STATUSES.map((s) => [s, []]));
    for (const task of tasksQuery.data ?? []) {
      if (isTaskStatus(task.status)) map.get(task.status)?.push(task);
    }
    return map;
  }, [tasksQuery.data]);

  const onDragEnd = (event: DragEndEvent): void => {
    const taskId = String(event.active.id);
    const overId = event.over?.id;
    if (typeof overId !== "string" || !isTaskStatus(overId)) return;
    const task = (tasksQuery.data ?? []).find((x) => x.id === taskId);
    if (task === undefined || task.status === overId) return;
    moveMut.mutate({ taskId, status: overId });
  };

  return (
    <div className="flex h-full flex-col p-3">
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-sm font-semibold">{t("shell.navBoard")}</h2>
        <button
          type="button"
          onClick={() => setFormOpen((o) => !o)}
          aria-expanded={formOpen}
          className="rounded bg-ink-accent px-3 py-1 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent"
        >
          {t("board.newTask")}
        </button>
      </div>
      {formOpen && <NewTaskForm onDone={() => setFormOpen(false)} />}
      <div className="min-h-0 flex-1">
        <AsyncBoundary
          isLoading={tasksQuery.isLoading}
          error={tasksQuery.error}
          isEmpty={(tasksQuery.data?.length ?? 0) === 0}
          onRetry={() => void tasksQuery.refetch()}
        >
          <DndContext onDragEnd={onDragEnd}>
            <div className="grid h-full grid-cols-5 gap-2">
              {TASK_STATUSES.map((status) => (
                <BoardColumn
                  key={status}
                  status={status}
                  tasks={grouped.get(status) ?? []}
                  onOpenRuns={setRunDrawerTask}
                  onMove={(taskId, next) => moveMut.mutate({ taskId, status: next })}
                />
              ))}
            </div>
          </DndContext>
        </AsyncBoundary>
      </div>
      <RunDrawer />
    </div>
  );
}
