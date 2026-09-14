import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { ipc, type ScheduleDto } from "../../../lib/ipc/client";
import { toast } from "../../../lib/store/toastStore";
import { describeError } from "../../../i18n";

export interface ScheduleRuleCardProps {
  schedule: ScheduleDto;
}

/** Schedule rule card with cron display, enable/disable toggle, and edit. */
export function ScheduleRuleCard({ schedule }: ScheduleRuleCardProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();

  const toggle = async (): Promise<void> => {
    try {
      await ipc.toggleSchedule(schedule.id, !schedule.enabled);
      void qc.invalidateQueries({ queryKey: ["schedules"] });
    } catch (e) {
      toast.error(describeError(e));
    }
  };

  return (
    <div className="rounded border border-ink-muted/30 p-2">
      <div className="flex items-center justify-between">
        <span className="text-sm font-medium text-ink">{schedule.name}</span>
        <button
          type="button"
          onClick={() => void toggle()}
          className={`rounded px-1.5 py-0.5 text-xs ${
            schedule.enabled
              ? "bg-state-ok/20 text-state-ok"
              : "bg-ink-muted/20 text-ink-muted"
          }`}
        >
          {schedule.enabled ? t("conversation.enabled") : t("conversation.disabled")}
        </button>
      </div>
      <div className="mt-1 text-xs text-ink-muted">
        <span className="font-mono">{schedule.cronExpr}</span>
        {" — "}
        <span>{schedule.taskTitle}</span>
      </div>
    </div>
  );
}
