import type { ReactNode } from "react";
import { Dialog } from "../../components/ui/Dialog";

export interface ClearConfirmDialogProps {
  open: boolean;
  title: string;
  description: string;
  onConfirm: () => void;
  onClose: () => void;
}

export function ClearConfirmDialog({
  open,
  title,
  description,
  onConfirm,
  onClose,
}: ClearConfirmDialogProps): ReactNode {
  return (
    <Dialog
      open={open}
      title={title}
      onClose={onClose}
      footer={
        <>
          <button
            type="button"
            className="sketch-btn px-3 py-1 text-sm text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
            onClick={onClose}
          >
            取消
          </button>
          <button
            type="button"
            className="sketch-btn px-3 py-1 text-sm text-state-danger focus-visible:ring-2 focus-visible:ring-ink-accent"
            onClick={() => {
              onConfirm();
              onClose();
            }}
          >
            确认
          </button>
        </>
      }
    >
      <p className="mb-2 text-sm text-ink">{description}</p>
      <p className="text-xs text-state-danger">此操作不可撤销</p>
    </Dialog>
  );
}
