import type { ReactNode } from "react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { DndContext, type DragEndEvent } from "@dnd-kit/core";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { DOMAIN_CHANNEL } from "../../lib/events/types";
import { useDomainEvents } from "../../lib/events/useDomainEvents";
import { describeError } from "../../i18n";
import type { TaskDto, TeamPlanDto } from "../../lib/ipc/bindings.gen";
import { IpcCommandError, ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";
import { AutoFormConfirmDialog } from "./AutoFormConfirmDialog";
import { BoardColumn } from "./BoardColumn";
import { NewTaskForm } from "./NewTaskForm";
import { RunDrawer } from "./RunDrawer";
import { useRunAllQueued } from "./useRunAllQueued";
import { isTaskStatus, TASK_STATUSES, type TaskStatus } from "./taskStatuses";
import { ViewScopeToggle } from "../common/ViewScopeToggle";
import { useViewScope } from "../common/useViewScope";

/** U11 kanban: five status columns, drag or menu to move, run drawer. */
export function BoardView(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [formOpen, setFormOpen] = useState(false);
  const [planConfirm, setPlanConfirm] = useState<
    { taskId: string; taskTitle: string; plan: TeamPlanDto } | null
  >(null);
  const setRunDrawerTask = useUiStore((s) => s.setRunDrawerTask);
  const focusedWorkspaceId = useUiStore((s) => s.focusedWorkspaceId);
  const { scope, setScope } = useViewScope("board");
  const workspaceFilter = scope === "focused" ? focusedWorkspaceId : null;
  const tasksQuery = useQuery({ queryKey: ["tasks", null, workspaceFilter], queryFn: () => ipc.listTasks(null, workspaceFilter) });

  const moveMut = useMutation({
    mutationFn: ({ taskId, status }: { taskId: string; status: TaskStatus }) =>
      ipc.updateTaskStatus(taskId, status),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["tasks"] }),
    onError: (e) => toast.error(`${t("board.moveFailed")}: ${describeError(e)}`),
  });

  const deleteMut = useMutation({
    mutationFn: (taskId: string) => ipc.deleteTask(taskId),
    onSuccess: (_data, taskId) => {
      void qc.invalidateQueries({ queryKey: ["tasks"] });
      void qc.invalidateQueries({ queryKey: ["runs", taskId] });
      toast.success(t("board.deleted"));
    },
    onError: (e) => {
      if (e instanceof IpcCommandError && e.code === "task.invalid_status") {
        toast.error(t("board.taskRunningDeleteHint"));
        return;
      }
      toast.error(`${t("board.deletedFailed")}: ${describeError(e)}`);
    },
  });

  /** 批次二② run-all: dispatch every queued task one by one; a single
   * failure never stops the rest and the summary toast reports both. */
  const { runAllPending, runAllQueued } = useRunAllQueued(qc);

  const runWithTeamMut = useMutation({
    mutationFn: ({ taskId, teamId }: { taskId: string; teamId: string }) =>
      ipc.runTeamOnTask(taskId, teamId),
    onSuccess: (run, vars) => {
      void qc.invalidateQueries({ queryKey: ["runs", vars.taskId] });
      void qc.invalidateQueries({ queryKey: ["tasks"] });
      toast.success(t("board.runWithTeamStarted", { runId: run.id }));
    },
    onError: (e) => toast.error(`${t("board.runWithTeamFailed")}: ${describeError(e)}`),
  });

  const taskText = (taskId: string): string => {
    const task = (tasksQuery.data ?? []).find((x) => x.id === taskId);
    return [task?.title ?? "", task?.description ?? ""].filter(Boolean).join("\n");
  };

  // Step 1 of 自发组队: dry-run preview only — no roles/teams are touched yet.
  const previewTeamMut = useMutation({
    mutationFn: async ({ taskId }: { taskId: string }) => {
      const task = (tasksQuery.data ?? []).find((x) => x.id === taskId);
      const plan = await ipc.previewTeam(taskText(taskId));
      return { taskId, taskTitle: task?.title ?? "", plan };
    },
    onSuccess: ({ taskId, taskTitle, plan }) => setPlanConfirm({ taskId, taskTitle, plan }),
    onError: (e) => toast.error(`${t("board.autoFormFailed")}: ${describeError(e)}`),
  });

  // Step 2 of 自发组队: the user confirmed — form the team and run it.
  const autoFormRunMut = useMutation({
    mutationFn: async ({ taskId }: { taskId: string }) => {
      const team = await ipc.formTeam(taskText(taskId), null);
      const run = await ipc.runTeamOnTask(taskId, team.id);
      return { team, run };
    },
    onSuccess: ({ team, run }, vars) => {
      void qc.invalidateQueries({ queryKey: ["runs", vars.taskId] });
      void qc.invalidateQueries({ queryKey: ["tasks"] });
      void qc.invalidateQueries({ queryKey: ["teams"] });
      toast.success(
        t("board.autoFormStarted", { team: team.name, members: team.memberRoleIds.length, run: run.id }),
      );
    },
    onError: (e) => toast.error(`${t("board.autoFormFailed")}: ${describeError(e)}`),
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

  const queuedIds = (grouped.get("queued") ?? []).map((task) => task.id);

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
        <div className="flex items-center gap-3">
          <h2 className="text-sm font-semibold">{t("shell.navBoard")}</h2>
          <ViewScopeToggle surface="board" scope={scope} onScopeChange={setScope} />
        </div>
        <button
          type="button"
          onClick={() => setFormOpen((o) => !o)}
          aria-expanded={formOpen}
          className="pixel-fill-accent px-3 py-1 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent"
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
                  scope={scope}
                  onOpenRuns={setRunDrawerTask}
                  onMove={(taskId, next) => moveMut.mutate({ taskId, status: next })}
                  onDelete={(taskId) => deleteMut.mutate(taskId)}
                  onRunAll={status === "queued" ? () => runAllQueued(queuedIds) : undefined}
                  runAllPending={runAllPending}
                  onRunWithTeam={(taskId, teamId) => runWithTeamMut.mutate({ taskId, teamId })}
                  onAutoFormRun={(taskId) => previewTeamMut.mutate({ taskId })}
                />
              ))}
            </div>
          </DndContext>
        </AsyncBoundary>
      </div>
      <RunDrawer />
      {planConfirm && (
        <AutoFormConfirmDialog
          taskTitle={planConfirm.taskTitle}
          plan={planConfirm.plan}
          onConfirm={() => {
            const { taskId } = planConfirm;
            setPlanConfirm(null);
            autoFormRunMut.mutate({ taskId });
          }}
          onCancel={() => setPlanConfirm(null)}
        />
      )}
    </div>
  );
}
