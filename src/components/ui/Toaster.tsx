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
        <button
          key={toast.id}
          type="button"
          onClick={() => dismiss(toast.id)}
          className={`pointer-events-auto rounded border px-3 py-2 text-left text-sm shadow-lg focus:outline-none focus-visible:ring-2 focus-visible:ring-ink-accent ${
            toast.kind === "error"
              ? "border-state-danger bg-surface-overlay text-ink"
              : "border-state-ok bg-surface-overlay text-ink"
          }`}
        >
          <span className="sr-only">{toast.kind === "error" ? t("common.toastError") : t("common.toastSuccess")}: </span>
          {toast.message}
        </button>
      ))}
    </div>
  );
}
