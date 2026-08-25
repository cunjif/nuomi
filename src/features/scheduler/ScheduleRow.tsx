import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ScheduleDto } from "../../lib/ipc/bindings.gen";

interface ScheduleRowProps {
  schedule: ScheduleDto;
  pending: boolean;
  onToggle: (id: string, enabled: boolean) => void;
  onDelete: (id: string) => void;
}

/** One schedule row with enable toggle and delete. */
export function ScheduleRow({ schedule, pending, onToggle, onDelete }: ScheduleRowProps): ReactNode {
  const { t } = useTranslation();
  return (
    <li className="flex items-center justify-between gap-3 rounded border border-ink-muted/40 bg-surface-raised p-3">
      <div className="min-w-0">
        <p className="truncate text-sm font-medium text-ink">{schedule.name}</p>
        <p className="font-mono text-xs text-ink-accent">{schedule.cronExpr}</p>
        <p className="truncate text-xs text-ink-muted">
          {schedule.taskTitle} ·{" "}
          {schedule.nextTriggerAt === null
            ? t("scheduler.never")
            : new Date(schedule.nextTriggerAt).toLocaleString()}
        </p>
      </div>
      <div className="flex shrink-0 items-center gap-2">
        <button
          type="button"
          role="switch"
          aria-checked={schedule.enabled}
          disabled={pending}
          onClick={() => onToggle(schedule.id, !schedule.enabled)}
          className={`rounded px-2 py-1 text-xs focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50 ${
            schedule.enabled ? "bg-state-ok/20 text-state-ok" : "bg-surface-overlay text-ink-muted"
          }`}
        >
          {schedule.enabled ? t("common.enabled") : t("common.disabled")}
        </button>
        <button
          type="button"
          disabled={pending}
          onClick={() => onDelete(schedule.id)}
          className="rounded border border-state-danger px-2 py-1 text-xs text-state-danger hover:bg-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
        >
          {t("common.delete")}
        </button>
      </div>
    </li>
  );
}
