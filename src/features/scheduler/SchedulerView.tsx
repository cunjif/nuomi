import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { DOMAIN_CHANNEL } from "../../lib/events/types";
import { useDomainEvents } from "../../lib/events/useDomainEvents";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";
import { ScheduleForm } from "./ScheduleForm";
import { ScheduleRow } from "./ScheduleRow";
import { ViewScopeToggle } from "../common/ViewScopeToggle";
import { useViewScope } from "../common/useViewScope";

/** U13 scheduler management page. */
export function SchedulerView({ workspaceId }: { workspaceId: string | null }): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [formOpen, setFormOpen] = useState(false);
  const focusedWorkspaceId = useUiStore((s) => s.focusedWorkspaceId);
  const { scope, setScope } = useViewScope("scheduler");
  const effectiveWorkspaceId = workspaceId ?? focusedWorkspaceId;
  const workspaceFilter = scope === "focused" ? effectiveWorkspaceId : null;
  const schedulesQuery = useQuery({ queryKey: ["schedules", workspaceFilter], queryFn: () => ipc.listSchedules(workspaceFilter) });

  const toggleMut = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) => ipc.toggleSchedule(id, enabled),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["schedules"] }),
    onError: (e) => toast.error(`${t("scheduler.toggleFailed")}: ${describeError(e)}`),
  });
  const deleteMut = useMutation({
    mutationFn: (id: string) => ipc.deleteSchedule(id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["schedules"] }),
    onError: (e) => toast.error(`${t("scheduler.deleteFailed")}: ${describeError(e)}`),
  });

  useDomainEvents([DOMAIN_CHANNEL], (batch) => {
    if (batch.some((e) => e.type.startsWith("schedule."))) {
      void qc.invalidateQueries({ queryKey: ["schedules"] });
    }
  });

  return (
    <div className="h-full overflow-y-auto p-3">
      <div className="mb-2 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h2 className="text-sm font-semibold">{t("scheduler.heading")}</h2>
          <ViewScopeToggle surface="scheduler" scope={scope} onScopeChange={setScope} />
        </div>
        <button
          type="button"
          onClick={() => setFormOpen((o) => !o)}
          aria-expanded={formOpen}
          className="pixel-fill-accent px-3 py-1 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent"
        >
          {t("common.create")}
        </button>
      </div>
      {formOpen && <ScheduleForm onDone={() => setFormOpen(false)} />}
      <AsyncBoundary
        isLoading={schedulesQuery.isLoading}
        error={schedulesQuery.error}
        isEmpty={(schedulesQuery.data?.length ?? 0) === 0}
        emptyLabel={t("scheduler.empty")}
        onRetry={() => void schedulesQuery.refetch()}
      >
        <ul className="flex flex-col gap-2">
          {(schedulesQuery.data ?? []).map((schedule) => (
            <ScheduleRow
              key={schedule.id}
              schedule={schedule}
              scope={scope}
              pending={toggleMut.isPending || deleteMut.isPending}
              onToggle={(id, enabled) => toggleMut.mutate({ id, enabled })}
              onDelete={(id) => deleteMut.mutate(id)}
            />
          ))}
        </ul>
      </AsyncBoundary>
    </div>
  );
}
