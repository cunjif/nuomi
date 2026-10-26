/**
 * Workspace create dialog (ADR-0024 Part A-f): lets the user name a new
 * workspace. Directory binding is NOT required at creation time — the
 * workspace starts empty and directories are added afterwards via the
 * WorkspaceBottomBar + button or drag-drop. When `initialDir` is provided
 * (drag-drop onto empty state), the directory is auto-added after creation.
 */
import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";

export function WorkspaceCreateDialog({
  open,
  initialDir,
  onClose,
}: {
  open: boolean;
  initialDir?: string | null;
  onClose: () => void;
}): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [name, setName] = useState("");

  if (!open) return null;

  const invalidate = (): void => {
    void qc.invalidateQueries({ queryKey: ["workspaces"] });
    void qc.invalidateQueries({ queryKey: ["workspace"] });
    void qc.invalidateQueries({ queryKey: ["dir"] });
  };

  const handleCreate = async (): Promise<void> => {
    const trimmed = name.trim();
    if (!trimmed) {
      toast.error(t("workspace.create.nameRequired"));
      return;
    }
    try {
      const id = await useUiStore.getState().createWorkspace(trimmed);
      await useUiStore.getState().switchToWorkspace(id);
      // If a directory was pre-filled (drag-drop onto empty state), add it
      // to the newly created workspace now that one is open.
      if (initialDir) {
        await useUiStore.getState().openDirectory(initialDir);
      }
      invalidate();
      toast.success(t("workspace.create.created"));
      useUiStore.getState().clearPendingCreateWorkspaceDir();
      onClose();
    } catch (e) {
      toast.error(`${t("workspace.create.failed")}: ${String(e)}`);
    }
  };

  const handleCancel = (): void => {
    useUiStore.getState().clearPendingCreateWorkspaceDir();
    onClose();
  };

  return (
    <div role="dialog" aria-label={t("workspace.create.title")} className="fixed inset-0 z-50 flex items-center justify-center">
      <div aria-hidden="true" className="absolute inset-0 bg-surface-scrim" onMouseDown={(e) => { e.preventDefault(); handleCancel(); }} />
      <div data-testid="workspace-create-dialog" className="sketch-panel relative w-full max-w-md border border-ink-muted/40 bg-surface-raised p-4 shadow-2xl">
        <h2 className="mb-3 text-title-hand text-sm font-semibold">{t("workspace.create.title")}</h2>
        <label className="mb-3 block text-xs text-ink-muted">
          {t("workspace.create.nameLabel")}
          <input
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            autoFocus
            placeholder={t("workspace.create.namePlaceholder")}
            className="mt-1 w-full rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
          />
        </label>
        <div className="flex justify-end gap-2">
          <button type="button" onClick={handleCancel} className="sketch-btn px-3 py-1 text-xs text-ink-muted">
            {t("common.cancel")}
          </button>
          <button
            type="button"
            onClick={() => void handleCreate()}
            disabled={!name.trim()}
            className="rounded bg-ink-accent px-3 py-1 text-xs text-surface hover:opacity-90 disabled:opacity-40"
          >
            {t("workspace.create.confirm")}
          </button>
        </div>
      </div>
    </div>
  );
}
