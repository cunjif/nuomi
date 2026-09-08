/**
 * 切换工作区 modal dialog (用户 SVG 布局 one.svg):
 *
 *   ┌──────────────────────────────────────────┐
 *   │ 当前工作目录  [ readonly current root ]    │
 *   │ 新的工作目录  [ input            ] [选择]  │
 *   │                        [取消]  [保存]     │
 *   └──────────────────────────────────────────┘
 *
 * 选择 opens the OS folder picker via the Tauri dialog plugin (dynamic
 * import + graceful toast fallback when the picker is unavailable, e.g.
 * running under vitest or a stripped build). 保存 persists through
 * ipc.setWorkspace and invalidates the workspace/dir query caches.
 */
import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

export function WorkspaceDialog({
  open,
  initialRoot,
  onClose,
}: {
  open: boolean;
  initialRoot: string;
  onClose: () => void;
}): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [path, setPath] = useState(initialRoot);

  // initialRoot often arrives after the first render (workspace query still
  // loading); follow it while the dialog is open until the user edits.
  useEffect(() => {
    if (open) setPath(initialRoot);
  }, [open, initialRoot]);

  const switchMut = useMutation({
    mutationFn: (next: string) => ipc.setWorkspace(next),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["workspace"] });
      void qc.invalidateQueries({ queryKey: ["dir"] });
      toast.success(t("workspace.switched"));
      onClose();
    },
    onError: (e) => toast.error(`${t("workspace.setupFailed")}: ${describeError(e)}`),
  });

  const browse = async (): Promise<void> => {
    try {
      const { open: pickFolder } = await import("@tauri-apps/plugin-dialog");
      const picked = await pickFolder({ directory: true, multiple: false, title: t("workspace.choose") });
      if (typeof picked === "string" && picked.length > 0) setPath(picked);
    } catch {
      toast.error(t("workspace.pickUnavailable"));
    }
  };

  if (!open) return null;
  return (
    <div
      role="dialog"
      aria-label={t("workspace.switchWorkspace")}
      className="fixed inset-0 z-50 flex items-center justify-center"
    >
      {/* Backdrop: click dismisses (same recipe as QuickOpen). */}
      <div
        aria-hidden="true"
        className="absolute inset-0 bg-black/40"
        onMouseDown={(e) => {
          e.preventDefault();
          onClose();
        }}
      />
      <div
        data-testid="workspace-dialog"
        className="sketch-panel relative w-full max-w-md border border-ink-muted/40 bg-surface-raised p-4 shadow-2xl"
      >
        {/* Row 1: read-only current root (用户 SVG). */}
        <div className="flex items-center gap-2">
          <span className="shrink-0 text-xs text-ink-muted">{t("workspace.currentDir")}</span>
          <div
            className="min-w-0 flex-1 truncate rounded border border-ink-muted/40 bg-surface px-2 py-1 font-mono text-xs text-ink-muted"
            title={initialRoot}
          >
            {initialRoot}
          </div>
        </div>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (path.trim().length > 0) switchMut.mutate(path);
          }}
        >
          {/* Row 2: new directory input + OS folder picker. */}
          <div className="mt-3 flex items-center gap-2">
            <label htmlFor="workspace-new-dir" className="shrink-0 text-xs text-ink-muted">
              {t("workspace.newDir")}
            </label>
            <input
              id="workspace-new-dir"
              type="text"
              value={path}
              onChange={(e) => setPath(e.target.value)}
              className="min-w-0 flex-1 rounded border border-ink-muted/40 bg-surface px-2 py-1 font-mono text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
            />
            <button
              type="button"
              onClick={() => void browse()}
              className="sketch-btn shrink-0 px-2 py-1 text-xs text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
            >
              {t("workspace.choose")}
            </button>
          </div>
          {/* Footer: 取消 secondary, 保存 primary (用户 SVG 右下). */}
          <div className="mt-4 flex justify-end gap-2">
            <button
              type="button"
              onClick={onClose}
              className="sketch-btn px-3 py-1 text-xs text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
            >
              {t("common.cancel")}
            </button>
            <button
              type="submit"
              disabled={switchMut.isPending || path.trim().length === 0}
              className="pixel-fill-accent px-3 py-1 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
            >
              {t("workspace.save")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
