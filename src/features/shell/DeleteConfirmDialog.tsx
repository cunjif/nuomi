import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Dialog } from "../../components/ui/Dialog";

export interface DeleteConfirmDialogProps {
  open: boolean;
  paths: string[];
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * Delete confirmation dialog (ADR 0016 §7). Lists up to 10 paths, shows a
 * count summary, and warns about permanent deletion of folders.
 */
export function DeleteConfirmDialog({
  open,
  paths,
  onConfirm,
  onCancel,
}: DeleteConfirmDialogProps): ReactNode {
  const { t } = useTranslation();
  const visible = paths.slice(0, 10);
  const remaining = paths.length - visible.length;

  return (
    <Dialog
      open={open}
      title={
        paths.length === 1
          ? t("files.deleteConfirmTitle")
          : t("files.deleteConfirmCount", { count: paths.length })
      }
      onClose={onCancel}
      footer={
        <>
          <button
            type="button"
            onClick={onCancel}
            className="sketch-btn px-3 py-1 text-sm text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("files.cancelBtn")}
          </button>
          <button
            type="button"
            onClick={onConfirm}
            className="sketch-btn px-3 py-1 text-sm text-state-danger focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("files.deleteBtn")}
          </button>
        </>
      }
    >
      {visible.map((p) => (
        <div key={p} className="truncate font-mono text-xs text-ink-muted">
          {p}
        </div>
      ))}
      {remaining > 0 && (
        <div className="mt-1 text-xs text-ink-muted">… {remaining}</div>
      )}
      <p className="mt-3 text-xs text-state-danger">
        {t("files.deleteConfirmDirWarning")}
      </p>
    </Dialog>
  );
}
