import { useCallback } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { ipc } from "../../lib/ipc/client";
import type { WorkspaceEntryDto, WorkDirectoryDto } from "../../lib/ipc/bindings.gen";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";
import { useInvalidateWorkspaceQueries, useOpenWorkspace } from "../../lib/store/workspaceQueries";

export interface WorkDirectoryOps {
  /** The open workspace, or null while a switch is in flight. */
  open: WorkspaceEntryDto | null;
  mountedDirs: WorkDirectoryDto[];
  unmountedDirs: WorkDirectoryDto[];
  /** Unsaved edits under the ACTIVE directory (removal guard input). */
  dirtyActiveFiles: string[];
  invalidate: () => void;
  /** Focuses a mounted directory (stale/active directories are no-ops). */
  focusDir: (dir: WorkDirectoryDto) => void;
  /** Removes without the dirty guard — callers decide confirmation. */
  removeDir: (dir: WorkDirectoryDto) => Promise<void>;
  /** Mounts an unmounted directory. Resolves false on failure. */
  mountDir: (dir: WorkDirectoryDto) => Promise<boolean>;
  /** Browses for a new directory; resolves true only when one was added. */
  browseNewDir: () => Promise<boolean>;
}

/**
 * Shared work-directory operations for the two directory UIs (explorer
 * sidebar bottom bar + collapsed right-panel dock): focusing/removing/
 * mounting/browsing all move the sandbox root, so file/dir caches under the
 * old root must refetch (same keys, new resolution) — single home for that
 * invalidation set and for the toast/error wiring.
 */
export function useWorkDirectoryOps(workspaceId: string): WorkDirectoryOps {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const { invalidateAll } = useInvalidateWorkspaceQueries();
  const editorByWorkspace = useUiStore((s) => s.editorByWorkspace);
  const { data: open } = useOpenWorkspace();

  const invalidate = useCallback((): void => {
    invalidateAll();
    void qc.invalidateQueries({ queryKey: ["workspace"] });
    void qc.invalidateQueries({ queryKey: ["dir"] });
    void qc.invalidateQueries({ queryKey: ["file"] });
  }, [invalidateAll, qc]);

  const focusDir = useCallback(
    (dir: WorkDirectoryDto): void => {
      if (dir.isActive) return;
      if (!dir.available) {
        // Stale directory: block the switch and hint removal/relocation.
        toast.error(t("workspace.directoryMissingToast"));
        return;
      }
      void ipc
        .workspaceFocusDirectory(workspaceId, dir.id)
        .then(() => invalidate())
        .catch((e) => toast.error(`${t("workspace.dirOpFailed")}: ${String(e)}`));
    },
    [invalidate, t, workspaceId],
  );

  const removeDir = useCallback(
    async (dir: WorkDirectoryDto): Promise<void> => {
      try {
        const result = await ipc.workspaceRemoveDirectory(workspaceId, dir.id);
        if (result.workspaceClosed) {
          // Last directory removed → backend closed the workspace; return to
          // the empty state.
          useUiStore.getState().setActiveWorkspaceId(null);
        }
        invalidate();
        toast.success(t("workspace.dirRemoved"));
      } catch (e) {
        toast.error(`${t("workspace.dirOpFailed")}: ${String(e)}`);
      }
    },
    [invalidate, t, workspaceId],
  );

  const mountDir = useCallback(
    async (dir: WorkDirectoryDto): Promise<boolean> => {
      if (!dir.available) return false;
      try {
        await ipc.workspaceMountDirectory(workspaceId, dir.id);
        invalidate();
        toast.success(t("workspace.dirMounted"));
        return true;
      } catch (e) {
        toast.error(`${t("workspace.dirOpFailed")}: ${String(e)}`);
        return false;
      }
    },
    [invalidate, t, workspaceId],
  );

  const browseNewDir = useCallback((): Promise<boolean> => {
    return pickFolder({ directory: true, multiple: false })
      .then((selected) => {
        if (!selected) return false;
        // Joins the open workspace (and auto-focuses) via the two-level
        // add_directory semantics — no separate mount call needed.
        return useUiStore
          .getState()
          .openDirectory(selected)
          .then(() => {
            invalidate();
            toast.success(t("workspace.dirAdded"));
            return true;
          });
      })
      .catch((e) => {
        toast.error(`${t("workspace.addDirFailed")}: ${String(e)}`);
        return false;
      });
  }, [invalidate, t]);

  // Render only when the open workspace is the host (a mismatch means a
  // switch is in flight — hide until the query settles).
  const matched = open && open.id === workspaceId ? open : null;
  const dirs = matched?.directories ?? [];
  const dirtyFiles = Object.keys(editorByWorkspace[workspaceId]?.dirtyPaths ?? {}).filter(
    (p) => editorByWorkspace[workspaceId]?.dirtyPaths[p],
  );

  return {
    open: matched,
    mountedDirs: dirs.filter((d) => d.mounted),
    unmountedDirs: dirs.filter((d) => !d.mounted),
    dirtyActiveFiles: dirtyFiles,
    invalidate,
    focusDir,
    removeDir,
    mountDir,
    browseNewDir,
  };
}
