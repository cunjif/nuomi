import { useState, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { Icon } from "../../components/ui/Icon/Icon";
import { Dialog } from "../../components/ui/Dialog";
import { useUiStore } from "../../lib/store/uiStore";
import { WorkspaceListDialog } from "./WorkspaceListDialog";

interface WorkspaceEntryDto {
  id: string;
  rootPath: string;
  colorTag: string;
  createdAt: number;
  isActive: boolean;
  directoryPresent: boolean;
}

/**
 * Workspace bottom bar (ADR 0017 修订): lives at the bottom of the explorer
 * sidebar (FileTree aside). Shows all open workspaces as tabs with color
 * dots; right-click a tab for split / close. Replaces the top WorkspaceTabBar.
 *
 * `paneIndex` is `undefined` in single layout, 0/1 in split layout. Switching
 * workspace updates `activeWorkspaceId` (single) or the corresponding slot in
 * `splitWorkspaceIds` (split).
 */
export function WorkspaceBottomBar({
  workspaceId,
  paneIndex,
}: {
  workspaceId: string;
  paneIndex?: number;
}): ReactNode {
  const { t } = useTranslation();
  const openWorkspaceIds = useUiStore((s) => s.openWorkspaceIds);
  const pinnedWorkspaceIds = useUiStore((s) => s.pinnedWorkspaceIds);
  const switchWorkspace = useUiStore((s) => s.switchWorkspace);
  const focusWorkspace = useUiStore((s) => s.focusWorkspace);
  const closeWorkspace = useUiStore((s) => s.closeWorkspace);
  const pinWorkspace = useUiStore((s) => s.pinWorkspace);
  const unpinWorkspace = useUiStore((s) => s.unpinWorkspace);
  const editorByWorkspace = useUiStore((s) => s.editorByWorkspace);
  const splitWorkspaceIds = useUiStore((s) => s.splitWorkspaceIds);
  const setSplitWorkspaceIds = useUiStore((s) => s.setSplitWorkspaceIds);
  const setLayoutMode = useUiStore((s) => s.setLayoutMode);
  const [addOpen, setAddOpen] = useState(false);
  const [confirmCloseId, setConfirmCloseId] = useState<string | null>(null);
  const [menuForId, setMenuForId] = useState<string | null>(null);

  const { data: workspaces } = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => invoke<WorkspaceEntryDto[]>("list_workspaces"),
  });

  const wsMap = new Map((workspaces ?? []).map((w) => [w.id, w]));
  const openWs = openWorkspaceIds
    .map((id) => wsMap.get(id))
    .filter((w): w is WorkspaceEntryDto => w !== undefined);

  const handleActivate = (id: string): void => {
    if (paneIndex !== undefined && splitWorkspaceIds !== null) {
      const next: [string, string] = [...splitWorkspaceIds] as [string, string];
      next[paneIndex] = id;
      void setSplitWorkspaceIds(next);
      void focusWorkspace(id);
    } else {
      void focusWorkspace(id);
      switchWorkspace(id);
    }
  };

  const handleSplit = (id: string): void => {
    const other = workspaceId !== id ? workspaceId : (splitWorkspaceIds?.find((x) => x !== id) ?? openWorkspaceIds.find((x) => x !== id));
    if (other && other !== id) {
      void setSplitWorkspaceIds([other, id]);
      void setLayoutMode("split");
    }
  };

  const canSplit = (id: string): boolean => {
    const other = workspaceId !== id ? workspaceId : (splitWorkspaceIds?.find((x) => x !== id) ?? openWorkspaceIds.find((x) => x !== id));
    return other !== undefined && other !== id;
  };

  const handleClose = (id: string): void => {
    const dirtyPaths = editorByWorkspace[id]?.dirtyPaths ?? {};
    const dirtyFiles = Object.keys(dirtyPaths).filter((p) => dirtyPaths[p]);
    if (dirtyFiles.length > 0) setConfirmCloseId(id);
    else void closeWorkspace(id, false);
  };

  const confirmDirtyFiles = confirmCloseId !== null
    ? Object.keys(editorByWorkspace[confirmCloseId]?.dirtyPaths ?? {}).filter(
        (p) => editorByWorkspace[confirmCloseId]?.dirtyPaths[p],
      )
    : [];

  return (
    <div className="relative shrink-0 border-t border-ink-muted/30 bg-surface">
      <div
        className="flex items-center gap-0.5 overflow-x-auto px-1 py-1"
        role="tablist"
        aria-label={t("workspace.switchWorkspace")}
      >
        {openWs.map((ws) => {
          const active = workspaceId === ws.id;
          const pinned = pinnedWorkspaceIds.includes(ws.id);
          const name = ws.rootPath.split(/[/\\]/).pop() ?? ws.id;
          return (
            <div
              key={ws.id}
              className={`group relative flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[11px] transition-colors ${
                active ? "bg-surface-overlay text-ink" : "text-ink-muted hover:bg-surface-overlay"
              }`}
            >
              <button
                type="button"
                role="tab"
                aria-selected={active}
                onClick={() => handleActivate(ws.id)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenuForId(menuForId === ws.id ? null : ws.id);
                }}
                title={ws.rootPath}
                className="flex items-center gap-1 focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ink-accent rounded"
              >
                <span
                  className="h-2 w-2 shrink-0 rounded-full"
                  style={{ backgroundColor: `var(--color-${ws.colorTag})` }}
                  aria-hidden="true"
                />
                <span className="max-w-24 truncate">{name}</span>
                {pinned && <span className="text-ink-faint" aria-label="pinned">★</span>}
              </button>
              {!ws.directoryPresent && (
                <span className="text-state-warn" title={t("workspace.directoryMissing")}>!</span>
              )}
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  handleClose(ws.id);
                }}
                aria-label={`${t("common.close")} ${name}`}
                className="text-ink-muted opacity-0 hover:text-state-danger group-hover:opacity-100 focus-visible:opacity-100"
              >
                <Icon name="close" size={10} />
              </button>
              {menuForId === ws.id && (
                <div
                  className="absolute bottom-full left-0 z-10 mb-1 rounded border border-ink-muted/40 bg-surface-raised py-0.5 text-xs shadow-lg"
                  onMouseLeave={() => setMenuForId(null)}
                >
                  <button
                    type="button"
                    disabled={!canSplit(ws.id)}
                    onClick={() => {
                      handleSplit(ws.id);
                      setMenuForId(null);
                    }}
                    title={canSplit(ws.id) ? undefined : t("workspace.splitNeedsTwo")}
                    className="block w-full px-3 py-1 text-left text-ink-muted hover:bg-surface-overlay hover:text-ink disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent"
                  >
                    {t("workspace.splitOpen")}
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      if (pinned) void unpinWorkspace(ws.id);
                      else void pinWorkspace(ws.id);
                      setMenuForId(null);
                    }}
                    className="block w-full px-3 py-1 text-left text-ink-muted hover:bg-surface-overlay hover:text-ink"
                  >
                    {pinned ? t("workspace.unpin") : t("workspace.pin")}
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      handleClose(ws.id);
                      setMenuForId(null);
                    }}
                    className="block w-full px-3 py-1 text-left text-ink-muted hover:bg-surface-overlay hover:text-state-danger"
                  >
                    {t("common.close")}
                  </button>
                </div>
              )}
            </div>
          );
        })}
        <button
          type="button"
          onClick={() => setAddOpen(true)}
          aria-label={t("workspace.add")}
          title={t("workspace.add")}
          className="flex h-6 w-6 shrink-0 items-center justify-center rounded text-ink-muted transition-colors hover:bg-surface-overlay hover:text-ink-accent focus-visible:ring-1 focus-visible:ring-ink-accent"
        >
          +
        </button>
      </div>
      <WorkspaceListDialog open={addOpen} onClose={() => setAddOpen(false)} />
      <Dialog
        open={confirmCloseId !== null}
        title={t("workspace.closeDirtyTitle")}
        onClose={() => setConfirmCloseId(null)}
        footer={
          <>
            <button
              type="button"
              onClick={() => setConfirmCloseId(null)}
              className="rounded px-3 py-1 text-xs text-ink-muted hover:bg-surface-overlay"
            >
              {t("common.cancel")}
            </button>
            <button
              type="button"
              onClick={() => {
                if (confirmCloseId !== null) void closeWorkspace(confirmCloseId, true);
                setConfirmCloseId(null);
              }}
              className="rounded px-3 py-1 text-xs text-state-danger hover:bg-state-danger/10"
            >
              {t("workspace.closeDirtyConfirm")}
            </button>
          </>
        }
      >
        <p className="mb-2">{t("workspace.closeDirtyBody")}</p>
        <ul className="max-h-40 overflow-y-auto rounded bg-surface p-2 text-xs text-ink-muted">
          {confirmDirtyFiles.map((p) => (
            <li key={p} className="truncate font-mono">
              {p}
            </li>
          ))}
        </ul>
      </Dialog>
    </div>
  );
}
