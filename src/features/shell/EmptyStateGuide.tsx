import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useUiStore } from "../../lib/store/uiStore";
import { useRecentWorkspaces } from "../../lib/store/workspaceQueries";
import { WorkspaceCreateDialog } from "./WorkspaceCreateDialog";

/**
 * Empty-state guide: no open workspace. Offers workspace creation (ADR-0024
 * Part A) and a recent-workspace list rendered with the derived label
 * (ADR-0021, spec 5.5.1-2a). Migration-anomalous entries are marked ⚠ and
 * stay non-clickable (spec 5.5.3-2c) — aria-disabled instead of the disabled
 * attribute so the hover tooltip keeps working.
 */
export function EmptyStateGuide() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const { requestSwitchToWorkspace } = useUiStore();
  const { data: recentWorkspaces } = useRecentWorkspaces(10);
  const [createOpen, setCreateOpen] = useState(false);

  // Same invalidation set as WorkspaceListDialog: switching/opening changes
  // the active root, and listDir queries don't carry the workspace id, so
  // ["dir"]/["file"]/["workspace"] must be invalidated on every change.
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
          <h3 className="text-sm font-medium text-ink mb-2">{t("workspace.recentTitle")}</h3>
          <div className="flex flex-col gap-1">
            {recentWorkspaces.map((ws) => (
              <button
                key={ws.workspaceId}
                aria-disabled={ws.anomaly}
                onClick={() => {
                  if (!ws.anomaly) void handleSwitch(ws.workspaceId);
                }}
                title={ws.anomaly ? t("workspace.anomaly") : (ws.labelFull ?? undefined)}
                className={`flex items-center gap-2 px-3 py-2 rounded-md text-sm text-left ${
                  ws.anomaly ? "cursor-not-allowed opacity-50" : "hover:bg-surface/60"
                }`}
              >
                {ws.isPinned && <span className="text-xs text-ink-accent">★</span>}
                <span className="flex-1 truncate">{ws.name ?? ws.label ?? ws.workspaceId}</span>
                {ws.anomaly && <span className="text-xs text-state-warn">⚠</span>}
                <span className="text-xs text-ink-muted/60">
                  {new Date(ws.lastUsedAt).toLocaleDateString()}
                </span>
              </button>
            ))}
          </div>
        </div>
      )}
      <WorkspaceCreateDialog open={createOpen} onClose={() => setCreateOpen(false)} />
    </div>
  );
}
