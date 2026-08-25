import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

interface ScheduleFormProps {
  onDone: () => void;
}

/** Create-schedule form (name / cron / task title / description). */
export function ScheduleForm({ onDone }: ScheduleFormProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [cronExpr, setCronExpr] = useState("");
  const [taskTitle, setTaskTitle] = useState("");
  const [taskDescription, setTaskDescription] = useState("");

  const createMut = useMutation({
    mutationFn: () => ipc.createSchedule(name.trim(), cronExpr.trim(), taskTitle.trim(), taskDescription.trim()),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["schedules"] });
      setName("");
      setCronExpr("");
      setTaskTitle("");
      setTaskDescription("");
      onDone();
    },
    onError: (e) => toast.error(`${t("scheduler.createFailed")}: ${describeError(e)}`),
  });

  const field =
    "w-48 rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent";

  return (
    <form
      aria-label={t("scheduler.heading")}
      className="mb-2 flex flex-wrap items-end gap-2 rounded border border-ink-muted/40 bg-surface-raised p-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (name.trim() && cronExpr.trim() && taskTitle.trim()) createMut.mutate();
      }}
    >
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("scheduler.name")}
        <input value={name} onChange={(e) => setName(e.target.value)} required className={field} />
      </label>
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("scheduler.cronExpr")}
        <input value={cronExpr} onChange={(e) => setCronExpr(e.target.value)} required placeholder="0 9 * * *" className={`${field} font-mono`} />
      </label>
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("scheduler.taskTitle")}
        <input value={taskTitle} onChange={(e) => setTaskTitle(e.target.value)} required className={field} />
      </label>
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("scheduler.taskDescription")}
        <input value={taskDescription} onChange={(e) => setTaskDescription(e.target.value)} className="w-64 rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent" />
      </label>
      <button
        type="submit"
        disabled={createMut.isPending}
        className="rounded bg-ink-accent px-3 py-1.5 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      >
        {t("common.create")}
      </button>
      <button
        type="button"
        onClick={onDone}
        className="rounded border border-ink-muted px-3 py-1.5 text-sm text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
      >
        {t("common.cancel")}
      </button>
    </form>
  );
}
