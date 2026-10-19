import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Badge } from "../../components/ui/Badge";
import { Button } from "../../components/ui/Button";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import type { SandboxTaskDto } from "../../lib/ipc/bindings.gen";
import { toast } from "../../lib/store/toastStore";

type StatusTone = "ok" | "warn" | "danger" | "neutral";

export function toneForStatus(status: string): StatusTone {
  switch (status) {
    case "succeeded":
      return "ok";
    case "running":
    case "queued":
      return "warn";
    case "failed":
    case "timed_out":
    case "cancelled":
      return "danger";
    default:
      return "neutral";
  }
}

const TERMINAL = new Set(["succeeded", "failed", "timed_out", "cancelled"]);

/**
 * Task detail (ADR-0022 task 7.7): status badge, runtime / duration /
 * exit info, persisted stdout / stderr (monospace, scroll), artifact
 * location with a retry button when the return transfer failed, and the
 * cancel action for in-flight tasks.
 */
export function SandboxTaskDetail({ task }: { task: SandboxTaskDto }): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();

  const cancelMut = useMutation({
    mutationFn: () => ipc.sandboxCancelTask(task.id),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["sandbox"] });
      toast.success(t("sandbox.cancelRequested"));
    },
    onError: (e) => toast.error(describeError(e)),
  });

  const retryMut = useMutation({
    mutationFn: () => ipc.sandboxRetryArtifact(task.id),
    onSuccess: (updated) => {
      void qc.invalidateQueries({ queryKey: ["sandbox"] });
      if (updated.artifactRetryable) {
        toast.error(t("sandbox.artifactRetryPending"));
      } else {
        toast.success(t("sandbox.artifactRetryOk"));
      }
    },
    onError: (e) => toast.error(describeError(e)),
  });

  return (
    <div className="flex h-full flex-col gap-3 overflow-hidden">
      <div className="flex flex-wrap items-center gap-2">
        <Badge tone={toneForStatus(task.status)}>{t(`sandbox.status.${task.status}`)}</Badge>
        <span className="text-xs text-ink-muted">{task.runtime}</span>
        {task.durationMs !== null && (
          <span className="text-xs text-ink-muted">{t("sandbox.duration", { ms: task.durationMs })}</span>
        )}
        {task.usage?.cpuTimeMs != null && (
          <span className="text-xs text-ink-muted">{t("sandbox.usageCpu", { ms: task.usage.cpuTimeMs })}</span>
        )}
        {task.usage?.peakMemoryMb != null && (
          <span className="text-xs text-ink-muted">{t("sandbox.usageMemory", { mb: task.usage.peakMemoryMb })}</span>
        )}
        {task.exitInfo !== null && (
          <span className="font-mono text-xs text-ink-muted">{task.exitInfo}</span>
        )}
        {!TERMINAL.has(task.status) && (
          <Button
            variant="ghost"
            className="ml-auto"
            disabled={cancelMut.isPending}
            onClick={() => cancelMut.mutate()}
          >
            {t("sandbox.cancelTask")}
          </Button>
        )}
      </div>

      <section className="flex min-h-0 flex-1 flex-col gap-2">
        <h3 className="font-note-hand text-sm text-ink">stdout</h3>
        <pre
          aria-label="stdout"
          className="min-h-16 flex-1 overflow-auto rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-dashed border-ink-muted/40 bg-surface p-2 font-mono text-xs whitespace-pre-wrap text-ink"
        >
          {task.stdout === "" ? "—" : task.stdout}
        </pre>
        <h3 className="font-note-hand text-sm text-ink">stderr</h3>
        <pre
          aria-label="stderr"
          className="min-h-12 overflow-auto rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-dashed border-ink-muted/40 bg-surface p-2 font-mono text-xs whitespace-pre-wrap text-state-danger"
        >
          {task.stderr === "" ? "—" : task.stderr}
        </pre>
      </section>

      {(task.artifactRef !== null || task.artifactRetryable) && (
        <section className="flex flex-wrap items-center gap-2 border-t border-dashed border-ink-muted/30 pt-2">
          <span className="text-xs text-ink-muted">{t("sandbox.artifact")}</span>
          {task.artifactRef !== null ? (
            <code className="font-mono text-xs text-ink">{task.artifactRef}</code>
          ) : (
            <span className="text-xs text-state-warn">{t("sandbox.artifactMissing")}</span>
          )}
          {task.artifactRetryable && (
            <Button variant="ghost" disabled={retryMut.isPending} onClick={() => retryMut.mutate()}>
              {t("sandbox.artifactRetry")}
            </Button>
          )}
        </section>
      )}
    </div>
  );
}
