import { useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../../components/ui/Icon/Icon";
import { Dialog } from "../../components/ui/Dialog";
import { useClickOutside } from "../../components/ui/useClickOutside";
import type { WorkDirectoryDto } from "../../lib/ipc/bindings.gen";
import { useWorkDirectoryOps } from "./useWorkDirectoryOps";

/**
 * Workspace bottom bar (ADR-0021 two-level model): lives at the bottom of
 * the explorer sidebar and shows the open workspace's *mounted work
 * directories* as chips. Clicking an available chip focuses it (the
 * FileTree/sandbox root follows); a stale chip (missing on disk) toasts the
 * relocate hint instead (spec 5.5.3-1c). Right-click offers focus/remove;
 * the "+" popover mounts unmounted directories or browses for a new one.
 * Directory operations are shared with the collapsed-panel dock via
 * useWorkDirectoryOps — this component owns only its chip UI state.
 */
export function WorkspaceBottomBar({
  workspaceId,
}: {
  workspaceId: string;
}): ReactNode {
  const { t } = useTranslation();
  const ops = useWorkDirectoryOps(workspaceId);
  const [menuForId, setMenuForId] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [confirmRemoveId, setConfirmRemoveId] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  useClickOutside([rootRef], {
    enabled: menuForId !== null || addOpen,
    onOutside: () => {
      setMenuForId(null);
      setAddOpen(false);
    },
  });

  // Render only when the open workspace is this component's host (a mismatch
  // means a switch is in flight — hide until the query settles).
  if (ops.open === null) return null;

  const { mountedDirs, unmountedDirs } = ops;

  /**
   * Dirty guard only applies to the *active* directory: the editor can only
   * hold files under it (FileTree is rooted there), so removing another
   * directory can never discard unsaved edits.
   */
  const handleRemove = (dir: WorkDirectoryDto): void => {
    setMenuForId(null);
    if (dir.isActive && ops.dirtyActiveFiles.length > 0) {
      setConfirmRemoveId(dir.id);
      return;
    }
    void ops.removeDir(dir);
  };

  const confirmDirtyFiles = confirmRemoveId !== null ? ops.dirtyActiveFiles : [];

  return (
    <div ref={rootRef} className="relative shrink-0 border-t border-ink-muted/30 bg-surface">
      <div
        className="flex items-center gap-0.5 overflow-x-auto px-1 py-1"
        role="tablist"
        aria-label={t("workspace.workDirs")}
      >
        {mountedDirs.map((dir) => {
          const active = dir.isActive;
          return (
            <div key={dir.id} className="group relative flex shrink-0 items-center rounded">
              <button
                type="button"
                role="tab"
                aria-selected={active}
                onClick={() => ops.focusDir(dir)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenuForId(menuForId === dir.id ? null : dir.id);
                }}
                title={dir.absPath}
                className={`flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ink-accent ${
                  active
                    ? "bg-surface-overlay text-ink"
                    : dir.available
                      ? "text-ink-muted hover:bg-surface-overlay"
                      : "text-ink-muted/60 hover:bg-surface-overlay"
                }`}
              >
                <span className="max-w-24 truncate">{dir.displayName}</span>
                {!dir.available && (
                  <span className="text-state-warn" title={t("workspace.directoryMissing")}>
                    !
                  </span>
                )}
              </button>
              {menuForId === dir.id && (
                <div
                  className="absolute bottom-full left-0 z-10 mb-1 rounded border border-ink-muted/40 bg-surface-raised py-0.5 text-xs shadow-lg"
                  onMouseLeave={() => setMenuForId(null)}
                >
                  <button
                    type="button"
                    disabled={dir.isActive}
                    onClick={() => {
                      ops.focusDir(dir);
                      setMenuForId(null);
                    }}
                    className="block w-full px-3 py-1 text-left text-ink-muted hover:bg-surface-overlay hover:text-ink disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent"
                  >
                    {t("workspace.focusDir")}
                  </button>
                  <button
                    type="button"
                    onClick={() => handleRemove(dir)}
                    className="block w-full px-3 py-1 text-left text-ink-muted hover:bg-surface-overlay hover:text-state-danger"
                  >
                    {t("workspace.removeDir")}
                  </button>
                </div>
              )}
            </div>
          );
        })}
        <button
          type="button"
          onClick={() => setAddOpen((v) => !v)}
          aria-label={t("workspace.addDir")}
          title={t("workspace.addDir")}
          className="flex h-6 w-6 shrink-0 items-center justify-center rounded text-ink-muted transition-colors hover:bg-surface-overlay hover:text-ink-accent focus-visible:ring-1 focus-visible:ring-ink-accent"
        >
          <Icon name="plus" size={10} />
        </button>
      </div>
      {addOpen && (
        <div
          className="absolute bottom-full right-0 z-10 mb-1 min-w-44 rounded border border-ink-muted/40 bg-surface-raised py-1 text-xs shadow-lg"
          onMouseLeave={() => setAddOpen(false)}
        >
          {unmountedDirs.map((dir) => (
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
              className="block w-full px-3 py-1 text-left text-ink-muted hover:bg-surface-overlay hover:text-ink disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent"
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
            className="block w-full px-3 py-1 text-left text-ink-muted hover:bg-surface-overlay hover:text-ink-accent"
          >
            {t("workspace.browseNewDir")}
          </button>
        </div>
      )}
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
                const id = confirmRemoveId;
                setConfirmRemoveId(null);
                const dir = mountedDirs.find((d) => d.id === id);
                if (id !== null && dir !== undefined) void ops.removeDir(dir);
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
