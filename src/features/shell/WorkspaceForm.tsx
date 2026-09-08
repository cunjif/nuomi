import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

/**
 * Shared workspace path form: rendered by the first-launch setup page and the
 * FilePanel switch dialog. On success the workspace query (and the file tree)
 * is invalidated so consumers re-render against the new root.
 */
export function WorkspaceForm({
  initialRoot,
  onSuccess,
  onCancel,
}: {
  initialRoot: string;
  onSuccess?: () => void;
  onCancel?: () => void;
}): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [path, setPath] = useState(initialRoot);

  // `initialRoot` often arrives after the first render (workspace query still
  // loading); follow it until the user edits the field themselves.
  useEffect(() => {
    setPath(initialRoot);
  }, [initialRoot]);

  const switchMut = useMutation({
    mutationFn: (next: string) => ipc.setWorkspace(next),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["workspace"] });
      void qc.invalidateQueries({ queryKey: ["dir"] });
      onSuccess?.();
    },
    onError: (e) => toast.error(`${t("workspace.setupFailed")}: ${describeError(e)}`),
  });

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (path.trim().length > 0) switchMut.mutate(path);
      }}
      className="flex flex-col gap-2"
    >
      <label htmlFor="workspace-path" className="text-xs font-medium text-ink-muted">
        {t("workspace.pathLabel")}
      </label>
      <input
        id="workspace-path"
        type="text"
        value={path}
        onChange={(e) => setPath(e.target.value)}
        className="w-full rounded border border-ink-muted/40 bg-surface px-2 py-1.5 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
      />
      <div className="flex justify-end gap-2">
        {onCancel && (
          <button
            type="button"
            onClick={onCancel}
            className="rounded border border-ink-muted/40 px-3 py-1 text-xs text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("common.cancel")}
          </button>
        )}
        <button
          type="submit"
          disabled={switchMut.isPending}
          className="pixel-fill-accent px-3 py-1 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
        >
          {t("workspace.confirm")}
        </button>
      </div>
    </form>
  );
}
