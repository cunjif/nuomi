import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "../../components/ui/Badge";
import { Button } from "../../components/ui/Button";
import { EmptyState } from "../../components/ui/EmptyState";
import { Spinner } from "../../components/ui/Spinner";
import { DOMAIN_CHANNEL } from "../../lib/events/types";
import { useDomainEvents } from "../../lib/events/useDomainEvents";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { SandboxBuildForm } from "./SandboxBuildForm";
import { SandboxTaskDetail, toneForStatus } from "./SandboxTaskDetail";
import { SandboxTaskForm } from "./SandboxTaskForm";

/** How many recent tasks the list column fetches (ADR-0022 task 7.7). */
const TASK_LIST_LIMIT = 50;

/**
 * Sandbox view (ADR-0022 task 7.7): toolbar with the two entry points
 * ("run code" / "build app" dialogs), a newest-first task list on the left
 * and the selected task's detail (status, streams, artifact) on the right.
 * An empty runtime list means the sandbox plugin is not usable — the view
 * degrades to a setup hint instead of the work surface.
 */
export function SandboxView(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [runOpen, setRunOpen] = useState(false);
  const [buildOpen, setBuildOpen] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const runtimesQuery = useQuery({
    queryKey: ["sandbox", "runtimes"],
    queryFn: () => ipc.sandboxListRuntimes(),
  });
  const tasksQuery = useQuery({
    queryKey: ["sandbox", "tasks"],
    queryFn: () => ipc.sandboxListTasks(TASK_LIST_LIMIT),
  });

  // Kernel sandbox.* events (task state changes, artifact transfers) keep
  // both the list and the selected task's detail current.
  useDomainEvents([DOMAIN_CHANNEL], (batch) => {
    if (batch.some((e) => e.type.startsWith("sandbox."))) {
      void qc.invalidateQueries({ queryKey: ["sandbox"] });
    }
  });

  const runtimes = runtimesQuery.data ?? [];
  const tasks = tasksQuery.data ?? [];
  const selected = tasks.find((task) => task.id === selectedId) ?? null;

  const listSection = tasksQuery.isError ? (
    <EmptyState title={describeError(tasksQuery.error)} />
  ) : tasks.length === 0 ? (
    <EmptyState title={t("sandbox.empty")} />
  ) : (
    <ul className="flex flex-col gap-1">
      {tasks.map((task) => (
        <li key={task.id}>
          <button
            type="button"
            onClick={() => setSelectedId(task.id)}
            aria-current={selected?.id === task.id ? "true" : undefined}
            className={`flex w-full flex-col gap-1 rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-dashed border-ink-muted/40 p-2 text-left focus-visible:ring-2 focus-visible:ring-ink-accent ${
              selected?.id === task.id
                ? "bg-surface-overlay ring-1 ring-inset ring-ink-muted/40"
                : "bg-surface hover:bg-surface-overlay"
            }`}
          >
            <span className="flex items-center justify-between gap-2">
              <span className="font-mono text-xs text-ink">{task.id}</span>
              <Badge tone={toneForStatus(task.status)}>{t(`sandbox.status.${task.status}`)}</Badge>
            </span>
            <span className="text-xs text-ink-muted">
              {t(`sandbox.taskType.${task.taskType}`)} · {task.runtime}
            </span>
          </button>
        </li>
      ))}
    </ul>
  );

  return (
    <div className="flex h-full flex-col overflow-hidden p-3">
      <div className="mb-2 flex shrink-0 items-center justify-between">
        <h2 className="text-sm font-semibold">{t("sandbox.title")}</h2>
        <div className="flex gap-2">
          <Button size="sm" disabled={runtimes.length === 0} onClick={() => setRunOpen(true)}>
            {t("sandbox.newRun")}
          </Button>
          <Button
            size="sm"
            variant="outline"
            disabled={runtimes.length === 0}
            onClick={() => setBuildOpen(true)}
          >
            {t("sandbox.newBuild")}
          </Button>
        </div>
      </div>

      {runtimesQuery.isLoading ? (
        <div className="m-4 self-center">
          <Spinner />
        </div>
      ) : runtimesQuery.isError ? (
        <EmptyState title={describeError(runtimesQuery.error)} />
      ) : runtimes.length === 0 ? (
        <EmptyState title={t("sandbox.unavailable")} />
      ) : (
        <div className="flex min-h-0 flex-1 gap-2">
          <section
            aria-label={t("sandbox.listLabel")}
            className="w-2/5 min-w-0 overflow-y-auto pr-1"
          >
            {listSection}
          </section>
          <section aria-label={t("sandbox.detailLabel")} className="min-w-0 flex-1">
            {selected !== null ? (
              <SandboxTaskDetail task={selected} />
            ) : (
              <EmptyState title={t("sandbox.noSelection")} className="h-full" />
            )}
          </section>
        </div>
      )}

      {runOpen && (
        <SandboxTaskForm open={runOpen} onClose={() => setRunOpen(false)} runtimes={runtimes} />
      )}
      {buildOpen && (
        <SandboxBuildForm open={buildOpen} onClose={() => setBuildOpen(false)} runtimes={runtimes} />
      )}
    </div>
  );
}
