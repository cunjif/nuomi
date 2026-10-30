/**
 * Drag-drop handler for the temp workspace (workbench_enhance spec 5.2 /
 * design 2.2.2). Takes over Shell.tsx's onDragDropEvent: classifies each
 * dropped path as a file or directory, routes directories to the existing
 * workspace registration, and opens external files as editable temp tabs
 * (size-capped, de-duped, with read-error feedback).
 */
import { ipc } from "../../../lib/ipc/client";
import { useUiStore } from "../../../lib/store/uiStore";
import { toast } from "../../../lib/store/toastStore";
import { i18n } from "../../../i18n";
import { isEditorExtensionEnabled } from "../../../lib/editor-ext";
import { checkFileSize, classifyPath, isOutsideWorkspaces } from "./pathClassifier";

/** Extract the file name from an absolute path. */
function basename(absPath: string): string {
  return absPath.split(/[\\/]/).pop() ?? absPath;
}

/**
 * Handle a batch of dropped paths. Directories register as workspaces;
 * external files open as editable temp tabs; in-workspace files open via
 * the existing editor. Errors per-path are surfaced as toasts and never
 * abort the remaining paths.
 */
export async function handleDropEvent(paths: string[]): Promise<void> {
  const tempWorkspaceEnabled = isEditorExtensionEnabled("workbench.tempWorkspace");
  let workspaceRoots: string[] = [];
  try {
    const ws = await ipc.getWorkspace();
    workspaceRoots = ws.root !== "" ? [ws.root] : [];
  } catch {
    // No active workspace → every file is "outside".
  }

  for (const path of paths) {
    try {
      const kind = await classifyPath(path);
      if (kind === "directory") {
        await useUiStore.getState().openDirectory(path);
        continue;
      }

      // File: check if it's inside a workspace.
      if (!isOutsideWorkspaces(path, workspaceRoots)) {
        const wsId = useUiStore.getState().activeWorkspaceId;
        if (wsId !== null) {
          useUiStore.getState().openFile(path, wsId);
        }
        continue;
      }

      // External file: when temp workspace is disabled, skip (fall back to
      // the pre-enhancement behavior where only directories were handled).
      if (!tempWorkspaceEnabled) continue;

      // External file: size cap.
      const sizeCheck = await checkFileSize(path);
      if (!sizeCheck.ok) {
        toast.error(i18n.t("workbench.tempWorkspace.fileTooLarge"));
        continue;
      }

      // De-dup: focus if already open.
      const existing = useUiStore.getState().tempFileTabs.find((t) => t.absPath === path);
      if (existing !== undefined) {
        useUiStore.getState().focusTempFileTab(path);
        continue;
      }

      // Read + open as editable temp tab.
      const content = await ipc.readExternalFile(path);
      useUiStore.getState().openTempFileTab(path, basename(path), content);
    } catch {
      toast.error(i18n.t("workbench.tempWorkspace.readFailed"));
    }
  }
}
