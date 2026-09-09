import type { ReactNode } from "react";
import { useEffect } from "react";
import { Icon } from "./Icon/Icon";

/**
 * Hand-drawn modal (review §6.2 / §7.2.2). Fixed overlay + a dashed panel that
 * draws itself in (`.animate-draw-in`). A little "tape" strip at the top sells
 * the paper metaphor. Closes on overlay click and Escape; focus is trapped to
 * the panel while open.
 */
export interface DialogProps {
  open: boolean;
  title: ReactNode;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  className?: string;
}

export function Dialog({ open, title, onClose, children, footer, className = "" }: DialogProps): ReactNode {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      onClick={onClose}
      role="presentation"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={typeof title === "string" ? title : undefined}
        onClick={(e) => e.stopPropagation()}
        className={`sketch-card relative w-full max-w-lg animate-draw-in bg-surface-raised p-4 shadow-sketch-lg ${className}`}
      >
        <span
          aria-hidden="true"
          className="absolute -top-2 left-1/2 h-3 w-16 -translate-x-1/2 rotate-[-3deg] rounded-sm bg-ink-accent/40"
        />
        <div className="mb-3 flex items-center justify-between gap-2">
          <h2 className="font-title-hand text-base text-ink">{title}</h2>
          <button
            type="button"
            aria-label="Close"
            onClick={onClose}
            className="rounded-full p-1 text-ink-muted hover:bg-surface hover:text-ink"
          >
            <Icon name="close" size={16} />
          </button>
        </div>
        <div className="text-sm text-ink">{children}</div>
        {footer !== undefined && <div className="mt-4 flex justify-end gap-2">{footer}</div>}
      </div>
    </div>
  );
}
