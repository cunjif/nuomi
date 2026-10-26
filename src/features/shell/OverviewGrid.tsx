import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { useUiStore } from "../../lib/store/uiStore";
import { ipc } from "../../lib/ipc/client";
import type { WorkspaceEntryDto } from "../../lib/ipc/bindings.gen";
import { workspaceColorClass } from "./workspaceColors";

/**
 * Overview grid: monitoring surface over the open workspace (single-open,
 * ADR-0021). Cards render the derived label with the full label as the
 * secondary line — never the placeholder uuid root path (spec 5.5.1-2a).
 */
export function OverviewGrid() {
  const { t } = useTranslation();
  const { activeWorkspaceId, requestSwitchToWorkspace, setLayoutMode } = useUiStore();

  const { data: workspaces } = useQuery({
    queryKey: ["workspaces"],
    queryFn: ipc.listWorkspaces,
  });

  // Single-open (ADR-0021): the overview lists the open workspace only.
  const openWorkspaces = (workspaces ?? []).filter((w) =>
    w.id === activeWorkspaceId,
  );

  if (openWorkspaces.length === 0) return null;

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 px-6 py-2 text-xs text-ink-muted bg-surface/50 border-b border-divider">
        <span>{t("workspace.overviewHint")}</span>
      </div>
      <div className="grid grid-cols-2 lg:grid-cols-3 gap-4 p-6 flex-1 overflow-auto">
        {openWorkspaces.map((ws: WorkspaceEntryDto) => (
          <button
            key={ws.id}
            onClick={() => {
              // Guarded: leave overview only once the switch actually ran
              // (a parked action must not collapse the grid underneath it).
              if (requestSwitchToWorkspace(ws.id, ws.name ?? ws.label ?? ws.id)) {
                void setLayoutMode("single");
              }
            }}
            className="flex flex-col gap-2 p-4 rounded-lg border border-divider bg-surface hover:border-ink-accent/60 transition-colors text-left"
          >
            <div className="flex items-center gap-2">
              <span
                className={`w-3 h-3 rounded-full shrink-0 ${workspaceColorClass(ws.colorTag)}`}
              />
              <span className="font-medium truncate" title={ws.name ?? ws.labelFull ?? ws.rootPath}>
                {ws.name ?? ws.label ?? ws.id}
              </span>
            </div>
            <span className="text-xs text-ink-muted truncate">{ws.labelFull ?? ws.rootPath}</span>
            <div className="flex items-center gap-3 text-xs text-ink-muted">
              <span>
                {ws.directoryPresent
                  ? `✓ ${t("workspace.dirAccessible")}`
                  : `✗ ${t("workspace.directoryMissing")}`}
              </span>
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}
