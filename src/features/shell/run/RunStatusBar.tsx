/**
 * Lightweight run-status bar embedded at the bottom of the editor column.
 * Shows the latest run/build status (running / succeeded / failed) as a
 * single-line indicator. Clicking the bar switches to the terminal area
 * so the user can see the full output. Hidden when there are no recent
 * run tasks.
 */
import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { DOMAIN_CHANNEL } from "../../../lib/events/types";
import { useDomainEvents } from "../../../lib/events/useDomainEvents";
import { ipc } from "../../../lib/ipc/client";
import { useUiStore } from "../../../lib/store/uiStore";

type RunStatus = "running" | "succeeded" | "failed" | "idle";

interface RunStatusInfo {
  status: RunStatus;
  command: string;
  durationMs: number | null;
  exitCode: string | null;
}

const SUCCESS_AUTO_HIDE_MS = 3000;

export function RunStatusBar(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const setActiveArea = useUiStore((s) => s.setActiveArea);
  const [hidden, setHidden] = useState(false);

  const tasksQuery = useQuery({
    queryKey: ["sandbox", "tasks"],
    queryFn: () => ipc.sandboxListTasks(1),
  });

  useDomainEvents([DOMAIN_CHANNEL], (batch) => {
    if (batch.some((e) => e.type.startsWith("sandbox."))) {
      void qc.invalidateQueries({ queryKey: ["sandbox"] });
    }
  });

  const latestTask = tasksQuery.data?.[0] ?? null;

  let info: RunStatusInfo | null = null;
  if (latestTask !== null) {
    const status: RunStatus = latestTask.status === "succeeded"
      ? "succeeded"
      : latestTask.status === "failed" || latestTask.status === "timed_out" || latestTask.status === "cancelled"
        ? "failed"
        : latestTask.status === "running" || latestTask.status === "queued"
          ? "running"
          : "idle";
    info = {
      status,
      command: `${latestTask.runtime} · ${latestTask.taskType === "run_project" ? "build" : "run"}`,
      durationMs: latestTask.durationMs,
      exitCode: latestTask.exitInfo !== null ? latestTask.exitInfo : null,
    };
  }

  // Auto-hide on success after a delay.
  useEffect(() => {
    if (info?.status === "succeeded") {
      setHidden(false);
      const timer = setTimeout(() => setHidden(true), SUCCESS_AUTO_HIDE_MS);
      return () => clearTimeout(timer);
    }
    setHidden(false);
  }, [info?.status]);

  if (info === null || info.status === "idle" || hidden) {
    return null;
  }

  const toneClass =
    info.status === "running"
      ? "text-state-warn"
      : info.status === "succeeded"
        ? "text-state-ok"
        : "text-state-danger";

  const icon = info.status === "running" ? "●" : info.status === "succeeded" ? "✓" : "✗";

  const label =
    info.status === "running"
      ? t("workbench.run.statusRunning", { command: info.command })
      : info.status === "succeeded"
        ? t("workbench.run.statusSuccess", { duration: info.durationMs ?? 0 })
        : t("workbench.run.statusFailed", { code: info.exitCode ?? 1 });

  return (
    <div
      className="flex shrink-0 items-center justify-between gap-2 border-t border-ink-muted/30 bg-surface px-3 py-1 text-xs"
      data-testid="run-status-bar"
    >
      <span className={`flex items-center gap-1.5 ${toneClass}`}>
        <span aria-hidden="true">{icon}</span>
        <span>{label}</span>
      </span>
      <button
        type="button"
        onClick={() => setActiveArea("terminal")}
        className="text-ink-muted hover:text-ink-accent"
        aria-label={t("workbench.run.openTerminal")}
      >
        {t("workbench.run.openTerminal")} →
      </button>
    </div>
  );
}
