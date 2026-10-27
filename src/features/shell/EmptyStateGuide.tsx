import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useUiStore } from "../../lib/store/uiStore";
import { useRecentWorkspaces } from "../../lib/store/workspaceQueries";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { WorkspaceCreateDialog } from "./WorkspaceCreateDialog";
import { ClearConfirmDialog } from "../steward/ClearConfirmDialog";

/**
 * Empty-state guide: no open workspace. Offers workspace creation (ADR-0024
 * Part A) and a recent-workspace list rendered with the derived label
 * (ADR-0021, spec 5.5.1-2a). Migration-anomalous entries are marked ⚠ and
 * stay non-clickable (spec 5.5.3-2c) — aria-disabled instead of the disabled
 * attribute so the hover tooltip keeps working.
 *
 * Recent list items provide a per-item delete entry (spec 5.1.1) and the
 * section header offers a "clear all" action with confirmation (spec 5.2.1).
 */
export function EmptyStateGuide() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const { requestSwitchToWorkspace } = useUiStore();
  const { data: recentWorkspaces } = useRecentWorkspaces(10);
  const [createOpen, setCreateOpen] = useState(false);
  const [clearConfirmOpen, setClearConfirmOpen] = useState(false);

  // Same invalidation set as WorkspaceListDialog: switching/opening changes
  // the active root, and listDir queries don't carry the workspace id, so
  // ["dir"]/["file"]/["workspace"] must be invalidated on every change.
  // ["workspaces"] covers the recent list (RECENT_WORKSPACES_KEY prefix).
  const invalidate = (): void => {
    void qc.invalidateQueries({ queryKey: ["workspaces"] });
    void qc.invalidateQueries({ queryKey: ["workspace"] });
    void qc.invalidateQueries({ queryKey: ["sessions"] });
    void qc.invalidateQueries({ queryKey: ["dir"] });
    void qc.invalidateQueries({ queryKey: ["file"] });
  };

  const handleSwitch = (id: string): void => {
    // Empty state: no active workspace → the guard always passes.
    if (requestSwitchToWorkspace(id)) void invalidate();
  };

  const handleRemoveRecent = async (workspaceId: string): Promise<void> => {
    try {
      await ipc.removeRecentWorkspace(workspaceId);
      invalidate();
      toast.success(t("workspace.recentRemoved"));
    } catch {
      toast.error(t("workspace.recentRemoveFailed"));
    }
  };

  const handleClearRecent = async (): Promise<void> => {
    try {
      await ipc.clearRecentWorkspaces();
      invalidate();
      toast.success(t("workspace.recentCleared"));
    } catch {
      toast.error(t("workspace.recentClearFailed"));
    }
  };

  return (
    <div className="flex flex-col items-center justify-center h-full gap-6 p-8">
      <div className="text-center">
        <h2 className="text-xl font-medium text-ink mb-2">{t("workspace.emptyTitle")}</h2>
        <p className="text-sm text-ink-muted">{t("workspace.emptyBody")}</p>
      </div>
      <button
        onClick={() => setCreateOpen(true)}
        className="px-4 py-2 rounded-md bg-ink-accent text-surface text-sm hover:opacity-90"
      >
        {t("workspace.addNew")}
      </button>
      {recentWorkspaces && recentWorkspaces.length > 0 && (
        <div className="w-full max-w-md">
          <div className="flex items-center justify-between mb-2">
            <h3 className="text-sm font-medium text-ink">{t("workspace.recentTitle")}</h3>
            <button
              onClick={() => setClearConfirmOpen(true)}
              className="text-xs text-ink-muted hover:text-state-danger focus-visible:ring-2 focus-visible:ring-ink-accent rounded px-1"
            >
              {t("workspace.recentClear")}
            </button>
          </div>
          <div className="flex flex-col gap-1">
            {recentWorkspaces.map((ws) => (
              <div
                key={ws.workspaceId}
                className={`group flex items-center gap-2 px-3 py-2 rounded-md text-sm ${
                  ws.anomaly ? "cursor-not-allowed opacity-50" : "hover:bg-surface/60"
                }`}
              >
                <button
                  aria-disabled={ws.anomaly}
                  onClick={() => {
                    if (!ws.anomaly) void handleSwitch(ws.workspaceId);
                  }}
                  title={ws.anomaly ? t("workspace.anomaly") : (ws.labelFull ?? undefined)}
                  className="flex items-center gap-2 flex-1 min-w-0 text-left"
                >
                  {ws.isPinned && <span className="text-xs text-ink-accent">★</span>}
                  <span className="flex-1 truncate">{ws.name ?? ws.label ?? ws.workspaceId}</span>
                  {ws.anomaly && <span className="text-xs text-state-warn">⚠</span>}
                  <span className="text-xs text-ink-muted/60">
                    {new Date(ws.lastUsedAt).toLocaleDateString()}
                  </span>
                </button>
                <button
                  aria-label={t("workspace.recentRemove")}
                  title={t("workspace.recentRemove")}
                  onClick={(e) => {
                    e.stopPropagation();
                    void handleRemoveRecent(ws.workspaceId);
                  }}
                  className="opacity-0 group-hover:opacity-100 text-xs text-ink-muted hover:text-state-danger focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ink-accent rounded px-1 shrink-0"
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
        </div>
      )}
      <WorkspaceCreateDialog open={createOpen} onClose={() => setCreateOpen(false)} />
      <ClearConfirmDialog
        open={clearConfirmOpen}
        title={t("workspace.recentClearConfirmTitle")}
        description={t("workspace.recentClearConfirmBody")}
        onConfirm={() => {
          void handleClearRecent();
        }}
        onClose={() => setClearConfirmOpen(false)}
      />
    </div>
  );
}
