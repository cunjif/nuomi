/**
 * Workspace list dialog: a modal that shows all registered workspaces with
 * add/remove/switch actions. Replaces the legacy WorkspaceDialog's single-path
 * input with a full list view (spec 5.2.1 规则2 + 规则3).
 */
import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { open as pickFolder } from "@tauri-apps/plugin-dialog";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { Icon } from "../../components/ui/Icon/Icon";
import { useUiStore } from "../../lib/store/uiStore";
import type { WorkspaceEntryDto } from "../../lib/ipc/bindings.gen";

const COLOR_MAP: Record<string, string> = {
  "paper-yellow": "bg-amber-300",
  "ink-blue": "bg-blue-500",
  "moss-green": "bg-green-500",
  "brick-red": "bg-red-500",
  "violet": "bg-purple-500",
  "ochre-orange": "bg-orange-500",
  "slate-gray": "bg-slate-400",
  "pink-rose": "bg-pink-400",
};

function WorkspaceRow({
  ws,
  onActivate,
  onRemove,
}: {
  ws: WorkspaceEntryDto;
  onActivate: (id: string) => void;
  onRemove: (id: string) => void;
}): ReactNode {
  const { t } = useTranslation();
  return (
    <li
      className={`flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-xs ${
        ws.isActive ? "bg-surface-overlay text-ink-accent" : "text-ink-muted hover:bg-surface-overlay"
      }`}
      onClick={() => !ws.isActive && onActivate(ws.id)}
    >
      <span
        aria-hidden="true"
        className={`inline-block size-2.5 rounded-full border border-ink-muted/40 ${COLOR_MAP[ws.colorTag] ?? "bg-ink-muted"}`}
      />
      <span className="flex-1 truncate" title={ws.rootPath}>{ws.rootPath}</span>
      {ws.isActive && <span className="text-[10px] font-semibold text-ink-accent">●</span>}
      {!ws.directoryPresent && (
        <span className="text-[10px] text-state-warn" title={t("workspace.directoryMissing")}>⚠</span>
      )}
      <button
        type="button"
        aria-label={t("workspace.remove")}
        onClick={(e) => { e.stopPropagation(); onRemove(ws.id); }}
        className="rounded p-0.5 text-ink-muted/60 hover:bg-state-danger/20 hover:text-state-danger"
      >
        <Icon name="close" size={10} />
      </button>
    </li>
  );
}

export function WorkspaceListDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [confirmRemoveId, setConfirmRemoveId] = useState<string | null>(null);

  const { data: workspaces, isPending } = useQuery({
    queryKey: ["workspaces"],
    queryFn: ipc.listWorkspaces,
    enabled: open,
    staleTime: 5_000,
  });

  const invalidate = (): void => {
    void qc.invalidateQueries({ queryKey: ["workspaces"] });
    void qc.invalidateQueries({ queryKey: ["workspace"] });
    void qc.invalidateQueries({ queryKey: ["sessions"] });
    void qc.invalidateQueries({ queryKey: ["dir"] });
    void qc.invalidateQueries({ queryKey: ["file"] });
  };

  const handleAdd = async (): Promise<void> => {
    try {
      const selected = await pickFolder({ directory: true, multiple: false });
      if (!selected) return;
      const entry = await ipc.addWorkspace(selected);
      if (entry.isActive) {
        useUiStore.getState().switchWorkspace(entry.id);
      }
      invalidate();
      toast.success(t("workspace.added"));
    } catch (e) {
      toast.error(`${t("workspace.addFailed")}: ${String(e)}`);
    }
  };

  const handleActivate = async (id: string): Promise<void> => {
    try {
      await ipc.activateWorkspace(id);
      useUiStore.getState().switchWorkspace(id);
      invalidate();
      toast.success(t("workspace.switched"));
      onClose();
    } catch (e) {
      toast.error(`${t("workspace.switchFailed")}: ${String(e)}`);
    }
  };

  const handleRemove = async (id: string): Promise<void> => {
    try {
      await ipc.removeWorkspace(id);
      invalidate();
      toast.success(t("workspace.removed"));
    } catch (e) {
      toast.error(`${t("workspace.removeFailed")}: ${String(e)}`);
    } finally {
      setConfirmRemoveId(null);
    }
  };

  if (!open) return null;
  return (
    <div role="dialog" aria-label={t("workspace.switchWorkspace")} className="fixed inset-0 z-50 flex items-center justify-center">
      <div aria-hidden="true" className="absolute inset-0 bg-black/40" onMouseDown={(e) => { e.preventDefault(); onClose(); }} />
      <div data-testid="workspace-list-dialog" className="sketch-panel relative w-full max-w-lg border border-ink-muted/40 bg-surface-raised p-4 shadow-2xl">
        <div className="mb-3 flex items-center justify-between">
          <h2 className="text-title-hand text-sm font-semibold">{t("workspace.listTitle")}</h2>
          <button
            type="button"
            onClick={() => void handleAdd()}
            className="flex items-center gap-1 rounded border border-dashed border-ink-muted/40 px-2 py-1 text-xs text-ink-muted hover:bg-surface-overlay hover:text-ink-accent"
          >
            <Icon name="plus" size={10} />
            {t("workspace.addNew")}
          </button>
        </div>
        <div className="max-h-64 overflow-y-auto">
          {isPending ? (
            <p className="px-2 py-4 text-xs text-ink-muted">{t("workspace.loading")}</p>
          ) : !workspaces || workspaces.length === 0 ? (
            <p className="px-2 py-4 text-xs text-ink-muted">{t("workspace.empty")}</p>
          ) : (
            <ul className="flex flex-col gap-0.5">
              {workspaces.map((ws) => (
                <WorkspaceRow key={ws.id} ws={ws} onActivate={(id) => void handleActivate(id)} onRemove={(id) => setConfirmRemoveId(id)} />
              ))}
            </ul>
          )}
        </div>
        {confirmRemoveId && (
          <div className="mt-3 border-t border-ink-muted/30 pt-2">
            <p className="mb-2 text-xs text-ink">{t("workspace.removeConfirm")}</p>
            <div className="flex gap-2">
              <button type="button" onClick={() => void handleRemove(confirmRemoveId)} className="rounded bg-state-danger px-2 py-1 text-xs text-surface hover:opacity-80">{t("workspace.confirmRemove")}</button>
              <button type="button" onClick={() => setConfirmRemoveId(null)} className="rounded border border-ink-muted/40 px-2 py-1 text-xs text-ink-muted hover:bg-surface-overlay">{t("workspace.cancelRemove")}</button>
            </div>
          </div>
        )}
        <div className="mt-3 flex justify-end">
          <button type="button" onClick={onClose} className="sketch-btn px-3 py-1 text-xs text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent">{t("common.cancel")}</button>
        </div>
      </div>
    </div>
  );
}
