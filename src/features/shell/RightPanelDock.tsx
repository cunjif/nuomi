import { useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useUiStore } from "../../lib/store/uiStore";
import { useClickOutside } from "../../components/ui/useClickOutside";
import { Dialog } from "../../components/ui/Dialog";
import { hashPaletteColor } from "../conversation/avatarPalette";
import type { WorkDirectoryDto } from "../../lib/ipc/bindings.gen";
import { useWorkDirectoryOps } from "./useWorkDirectoryOps";
import { DockIconButton } from "./DockIconButton";
import { WorkspaceListDialog } from "./WorkspaceListDialog";
import { DirBrowsePopover } from "./DirBrowsePopover";

/**
 * Icon dock shown when the explorer sidebar is collapsed: the workspace's
 * mounted work directories as hash-colored initial badges (click = focus,
 * right-click = remove), a "+" popover to mount unmounted directories or
 * browse for a new one, workspace switch, and expand / fully-hide controls.
 * Directory operations are shared with the sidebar bottom bar via
 * useWorkDirectoryOps; the file tree itself only exists in the expanded
 * panel, so the expand icon doubles as its entry.
 */
export function RightPanelDock({ workspaceId }: { workspaceId: string }): ReactNode {
  const { t } = useTranslation();
  const ops = useWorkDirectoryOps(workspaceId);
  const setRightPanel = useUiStore((s) => s.setRightPanel);
  const [switchOpen, setSwitchOpen] = useState(false);
  const [addOpen, setAddOpen] = useState(false);
  const [menuForId, setMenuForId] = useState<string | null>(null);
  const [confirmRemoveId, setConfirmRemoveId] = useState<string | null>(null);
  const [browseForId, setBrowseForId] = useState<string | null>(null);
  const addBtnRef = useRef<HTMLButtonElement>(null);
  const addPopRef = useRef<HTMLDivElement>(null);
  // Ref of the badge whose browse popover is open (ref callback swaps per
  // badge), so re-clicking the same badge toggles instead of re-opening.
  const browseBtnRef = useRef<HTMLButtonElement | null>(null);
  useClickOutside([addBtnRef, addPopRef], {
    enabled: addOpen,
    onOutside: () => setAddOpen(false),
    escape: true,
  });

  /** Dirty guard only applies to the *active* directory: the editor can only
   * hold files under it, so removing another directory can never discard
   * unsaved edits (same rule as the sidebar bottom bar). */
  const handleRemove = (dir: WorkDirectoryDto): void => {
    setMenuForId(null);
    if (dir.isActive && ops.dirtyActiveFiles.length > 0) {
      setConfirmRemoveId(dir.id);
      return;
    }
    void ops.removeDir(dir);
  };

  /** Badge click = focus + browse: focusing moves the sandbox root to this
   * directory (the browse popover's FileTree lists the active root), so the
   * popover shows exactly what was clicked. Stale directories stay
   * popover-free — focusDir already toasts the removal/relocation hint. */
  const handleBrowseClick = (dir: WorkDirectoryDto): void => {
    ops.focusDir(dir);
    if (!dir.available) return;
    setBrowseForId(browseForId === dir.id ? null : dir.id);
  };

  const confirmDir = confirmRemoveId !== null ? ops.mountedDirs.find((d) => d.id === confirmRemoveId) : undefined;

  return (
    <aside
      aria-label={t("files.treeLabel")}
      data-testid="right-panel-dock"
      className="flex w-10 shrink-0 flex-col items-center gap-1 border-l border-ink-muted/30 bg-surface py-2"
    >
      <DockIconButton icon="home" label={t("workspace.switchWorkspace")} onClick={() => setSwitchOpen(true)} />
      {ops.mountedDirs.length > 0 && (
        <div className="my-1 h-px w-6 bg-ink-muted/30" aria-hidden="true" role="presentation" />
      )}
      <div role="tablist" aria-label={t("workspace.workDirs")} className="flex flex-col items-center gap-1">
        {ops.mountedDirs.map((dir) => {
          const color = hashPaletteColor(dir.displayName);
          const initial = dir.displayName.trim().charAt(0).toUpperCase() || "?";
          return (
            <div key={dir.id} className="relative flex items-center">
              <button
                type="button"
                role="tab"
                aria-selected={dir.isActive}
                aria-label={dir.displayName}
                title={dir.available ? dir.absPath : `${dir.displayName} — ${t("workspace.directoryMissing")}`}
                ref={browseForId === dir.id ? browseBtnRef : undefined}
                onClick={() => handleBrowseClick(dir)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenuForId(menuForId === dir.id ? null : dir.id);
                }}
                style={{ backgroundColor: color.bg, color: color.fg }}
                className={`flex size-7 items-center justify-center rounded text-xs font-semibold transition-opacity focus-visible:ring-2 focus-visible:ring-ink-accent ${
                  dir.isActive ? "ring-2 ring-ink-accent" : "opacity-70 hover:opacity-100"
                } ${dir.available ? "" : "opacity-40"}`}
              >
                {initial}
              </button>
              {browseForId === dir.id && (
                <DirBrowsePopover
                  workspaceId={workspaceId}
                  dir={dir}
                  buttonRef={browseBtnRef}
                  onClose={() => setBrowseForId(null)}
                />
              )}
              {menuForId === dir.id && (
                <div
                  className="absolute left-full top-0 z-30 ml-1 rounded border border-ink-muted/40 bg-surface-raised py-0.5 text-xs shadow-lg"
                  onMouseLeave={() => setMenuForId(null)}
                >
                  <button
                    type="button"
                    onClick={() => handleRemove(dir)}
                    className="block w-full whitespace-nowrap px-3 py-1 text-left text-ink-muted hover:bg-surface-overlay hover:text-state-danger"
                  >
                    {t("workspace.removeDir")}
                  </button>
                </div>
              )}
            </div>
          );
        })}
        <button
          ref={addBtnRef}
          type="button"
          onClick={() => setAddOpen((v) => !v)}
          aria-label={t("workspace.addDir")}
          title={t("workspace.addDir")}
          className="flex size-7 items-center justify-center rounded text-ink-muted transition-colors hover:bg-surface-overlay hover:text-ink-accent focus-visible:ring-1 focus-visible:ring-ink-accent"
        >
          +
        </button>
        {addOpen && (
          <div
            ref={addPopRef}
            data-testid="dock-add-dir-popover"
            className="absolute left-full bottom-0 z-30 ml-1 min-w-44 rounded border border-ink-muted/40 bg-surface-raised py-1 text-xs shadow-lg"
          >
            {ops.unmountedDirs.map((dir) => (
              <button
                key={dir.id}
                type="button"
                disabled={!dir.available}
                title={dir.available ? dir.absPath : t("workspace.directoryMissing")}
                onClick={() => {
                  void ops.mountDir(dir).then((added) => {
                    if (added) setAddOpen(false);
                  });
                }}
                className="block w-full whitespace-nowrap px-3 py-1 text-left text-ink-muted hover:bg-surface-overlay hover:text-ink disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent"
              >
                {dir.displayName}
                {!dir.available && <span className="text-state-warn"> !</span>}
              </button>
            ))}
            <div className="my-1 border-t border-ink-muted/30" />
            <button
              type="button"
              onClick={() => {
                void ops.browseNewDir().then((added) => {
                  if (added) setAddOpen(false);
                });
              }}
              className="block w-full whitespace-nowrap px-3 py-1 text-left text-ink-muted hover:bg-surface-overlay hover:text-ink-accent"
            >
              {t("workspace.browseNewDir")}
            </button>
          </div>
        )}
      </div>
      <div className="mt-auto flex flex-col items-center gap-1 pt-2">
        <DockIconButton icon="eye-off" label={t("shell.panelHide")} onClick={() => setRightPanel({ hidden: true })} />
        <DockIconButton icon="panel-right" label={t("shell.panelExpandRight")} onClick={() => setRightPanel({ collapsed: false })} />
      </div>
      {switchOpen && <WorkspaceListDialog open onClose={() => setSwitchOpen(false)} />}
      <Dialog
        open={confirmRemoveId !== null}
        title={t("workspace.removeDirDirtyTitle")}
        onClose={() => setConfirmRemoveId(null)}
        footer={
          <>
            <button
              type="button"
              onClick={() => setConfirmRemoveId(null)}
              className="rounded px-3 py-1 text-xs text-ink-muted hover:bg-surface-overlay"
            >
              {t("common.cancel")}
            </button>
            <button
              type="button"
              onClick={() => {
                const dir = confirmDir;
                setConfirmRemoveId(null);
                if (dir !== undefined) void ops.removeDir(dir);
              }}
              className="rounded px-3 py-1 text-xs text-state-danger hover:bg-state-danger/10"
            >
              {t("workspace.removeDirDirtyConfirm")}
            </button>
          </>
        }
      >
        <p className="mb-2">{t("workspace.removeDirDirtyBody")}</p>
        <ul className="max-h-40 overflow-y-auto rounded bg-surface p-2 text-xs text-ink-muted">
          {ops.dirtyActiveFiles.map((p) => (
            <li key={p} className="truncate font-mono">
              {p}
            </li>
          ))}
        </ul>
      </Dialog>
    </aside>
  );
}
