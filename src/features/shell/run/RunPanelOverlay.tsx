/**
 * Run-result overlay embedded at the bottom of the editor column
 * (workbench_enhance spec 5.1.1 / design 2.2.2). Lists sandbox tasks
 * started from the editor page, with per-task tabs, status badges
 * (toneForStatus six-state), independent cancel, and a collapse toggle
 * so the panel never blocks the editor body. Reuses SandboxTaskDetail
 * for the stdout/stderr/exit/duration/artifact rendering and the
 * sandbox.* domain events for real-time state sync.
 */
import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "../../../components/ui/Badge";
import { EmptyState } from "../../../components/ui/EmptyState";
import { DOMAIN_CHANNEL } from "../../../lib/events/types";
import { useDomainEvents } from "../../../lib/events/useDomainEvents";
import { ipc } from "../../../lib/ipc/client";
import { SandboxTaskDetail, toneForStatus } from "../../sandbox/SandboxTaskDetail";

const TASK_LIST_LIMIT = 50;

export function RunPanelOverlay({ workspaceId }: { workspaceId: string }): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [collapsed, setCollapsed] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const tasksQuery = useQuery({
    queryKey: ["sandbox", "tasks"],
    queryFn: () => ipc.sandboxListTasks(TASK_LIST_LIMIT),
  });

  useDomainEvents([DOMAIN_CHANNEL], (batch) => {
    if (batch.some((e) => e.type.startsWith("sandbox."))) {
      void qc.invalidateQueries({ queryKey: ["sandbox"] });
    }
  });

  const tasks = tasksQuery.data ?? [];
  // Auto-select the newest task when none is selected.
  useEffect(() => {
    if (selectedId === null && tasks.length > 0) {
      setSelectedId(tasks[0]?.id ?? null);
    }
  }, [selectedId, tasks]);

  const selected = tasks.find((task) => task.id === selectedId) ?? null;

  if (collapsed) {
    return (
      <div
        className="flex shrink-0 items-center gap-2 border-t border-ink-muted/30 bg-surface px-2 py-1"
        data-testid="run-panel-collapsed"
      >
        <button
          type="button"
          onClick={() => setCollapsed(false)}
          className="text-xs text-ink-muted hover:text-ink"
          aria-label={t("workbench.runPanel.expand")}
        >
          ▾ {t("workbench.runPanel.title")}
        </button>
        {tasks.length > 0 && (
          <Badge tone={toneForStatus(tasks[0]?.status ?? "")}>
            {t(`sandbox.status.${tasks[0]?.status ?? "queued"}`)}
          </Badge>
        )}
      </div>
    );
  }

  return (
    <div
      className="flex h-48 shrink-0 flex-col border-t border-ink-muted/30 bg-surface-raised"
      data-testid="run-panel-overlay"
      data-workspace={workspaceId}
    >
      <div className="flex shrink-0 items-center justify-between gap-2 border-b border-ink-muted/30 px-2 py-1">
        <span className="text-xs font-semibold uppercase tracking-wide text-ink-muted">
          {t("workbench.runPanel.title")}
        </span>
        <button
          type="button"
          onClick={() => setCollapsed(true)}
          className="text-xs text-ink-muted hover:text-ink"
          aria-label={t("workbench.runPanel.collapse")}
        >
          ▴
        </button>
      </div>
      {tasks.length === 0 ? (
        <EmptyState title={t("workbench.runPanel.empty")} className="flex-1" />
      ) : (
        <div className="flex min-h-0 flex-1">
          <div className="flex w-44 shrink-0 flex-col gap-1 overflow-y-auto border-r border-ink-muted/30 p-1">
            {tasks.map((task) => (
              <button
                key={task.id}
                type="button"
                onClick={() => setSelectedId(task.id)}
                aria-current={selected?.id === task.id ? "true" : undefined}
                className={`flex flex-col gap-0.5 rounded-sm border border-dashed border-ink-muted/40 p-1 text-left text-xs ${
                  selected?.id === task.id ? "bg-surface-overlay" : "bg-surface hover:bg-surface-overlay"
                }`}
              >
                <span className="flex items-center justify-between gap-1">
                  <span className="truncate font-mono text-[10px] text-ink">{task.id}</span>
                  <Badge tone={toneForStatus(task.status)}>{t(`sandbox.status.${task.status}`)}</Badge>
                </span>
                <span className="text-[10px] text-ink-muted">
                  {t(`sandbox.taskType.${task.taskType}`)} · {task.runtime}
                </span>
              </button>
            ))}
          </div>
          <div className="min-w-0 flex-1 overflow-hidden p-2">
            {selected !== null ? (
              <SandboxTaskDetail task={selected} />
            ) : (
              <EmptyState title={t("sandbox.noSelection")} className="h-full" />
            )}
          </div>
        </div>
      )}
    </div>
  );
}
