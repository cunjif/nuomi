import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useToastStore } from "../../lib/store/toastStore";

/** Fixed toast region; polite live region so screen readers announce. */
export function Toaster(): ReactNode {
  const { t } = useTranslation();
  const toasts = useToastStore((s) => s.toasts);
  const dismiss = useToastStore((s) => s.dismiss);
  return (
    <div role="status" aria-live="polite" className="pointer-events-none fixed bottom-4 right-4 z-50 flex flex-col gap-2">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className={`sketch-card sketch-shadow-md animate-draw-in pointer-events-auto flex items-center gap-3 border border-dashed px-3 py-2 text-left text-sm ${
            toast.kind === "error"
              ? "border-state-danger bg-surface-overlay text-ink"
              : toast.kind === "warn"
                ? "border-ink-accent bg-surface-overlay text-ink"
                : toast.kind === "info"
                  ? "border-ink/40 bg-surface-overlay text-ink"
                  : "border-state-ok bg-surface-overlay text-ink"
          }`}
        >
          <button
            type="button"
            onClick={() => dismiss(toast.id)}
            className="flex-1 focus:outline-none focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            <span className="sr-only">
              {toast.kind === "error"
                ? t("common.toastError")
                : toast.kind === "warn"
                  ? t("common.toastWarn")
                  : toast.kind === "info"
                    ? t("common.toastInfo")
                    : t("common.toastSuccess")}
              :{" "}
            </span>
            {toast.message}
          </button>
          {toast.action && (
            <button
              type="button"
              onClick={() => {
                toast.action?.onClick();
                dismiss(toast.id);
              }}
              className="shrink-0 border border-solid border-ink-accent px-2 py-0.5 text-xs font-bold text-ink-accent hover:bg-ink-accent/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-ink-accent"
            >
              {toast.action.label}
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
