import { useTranslation } from "react-i18next";
import type { ReactNode } from "react";
import { Dialog } from "../../components/ui/Dialog";
import { useUiStore, type PendingWorkspaceAction } from "../../lib/store/uiStore";

/**
 * Guard dialog for workspace-level actions (switch away / close). Two
 * guarded facts, two sections:
 *
 * - Unsaved edits (close AND switch): MonacoTab unmounts and reloads from
 *   disk, so unsaved input is discarded — the dialog makes that an explicit
 *   choice. No "save and continue" action on purpose: dirty content lives
 *   inside each MonacoTab's component state (unreachable from here), so the
 *   copy tells the user to Ctrl+S first.
 * - Active runs (close only — switching never kills runs): closing drops
 *   the watching surface while the runs keep going in the background.
 *
 * Cancel keeps everything as-is.
 */
export function WorkspaceGuardDialog(): ReactNode {
  const { t } = useTranslation();
  const pending = useUiStore((s) => s.pendingWorkspaceAction);
  const confirm = useUiStore((s) => s.confirmPendingWorkspaceAction);
  const cancel = useUiStore((s) => s.cancelPendingWorkspaceAction);

  if (pending === null) return null;

  const confirmLabel =
    pending.kind === "switch"
      ? t("workspace.guard.discardAndSwitch")
      : pending.dirtyFiles.length > 0
        ? t("workspace.guard.discardAndClose")
        : t("workspace.guard.stillClose");

  return (
    <Dialog
      open
      onClose={cancel}
      title={titleFor(pending, t)}
      footer={
        <>
          <button
            type="button"
            onClick={cancel}
            className="sketch-btn px-3 py-1.5 text-sm text-ink"
          >
            {t("workspace.guard.cancel")}
          </button>
          <button
            type="button"
            onClick={() => void confirm()}
            className="pixel-fill-warn px-3 py-1.5 text-sm text-surface shadow-sketch-sm"
          >
            {confirmLabel}
          </button>
        </>
      }
    >
      <GuardBody pending={pending} />
    </Dialog>
  );
}

function titleFor(
  pending: PendingWorkspaceAction,
  t: (key: string) => string,
): string {
  return pending.kind === "switch"
    ? t("workspace.guard.switchTitle")
    : t("workspace.guard.closeTitle");
}

function GuardBody({ pending }: { pending: PendingWorkspaceAction }): ReactNode {
  const { t } = useTranslation();
  const hasDirty = pending.dirtyFiles.length > 0;
  return (
    <div className="text-sm">
      {hasDirty && (
        <>
          <p className="text-ink-muted">
            {t("workspace.guard.body", { label: pending.workspaceLabel })}
          </p>
          <ul className="mt-2 max-h-32 overflow-auto rounded border border-divider bg-surface-overlay/50 p-2">
            {pending.dirtyFiles.map((f) => (
              <li key={f} className="truncate font-mono text-xs text-ink-muted">
                {f}
              </li>
            ))}
          </ul>
          <p className="mt-2 text-xs text-ink-muted">
            {t("workspace.guard.saveHint")}
          </p>
        </>
      )}
      {pending.runningTasks.length > 0 && (
        <>
          <p className={hasDirty ? "mt-4 text-ink-muted" : "text-ink-muted"}>
            {t("workspace.guard.runningBody")}
          </p>
          <ul className="mt-2 max-h-32 overflow-auto rounded border border-divider bg-surface-overlay/50 p-2">
            {pending.runningTasks.map((r) => (
              <li key={r.runId} className="truncate text-xs text-ink-muted">
                {r.taskTitle}
                <span className="ml-2 font-mono text-[10px] uppercase opacity-70">
                  {r.status}
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}
