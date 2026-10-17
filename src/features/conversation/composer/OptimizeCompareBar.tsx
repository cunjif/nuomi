import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

export interface OptimizeCompareBarProps {
  original: string;
  optimized: string;
  /** True while a re-optimize call is in flight (disables the retry button). */
  busy: boolean;
  onAdopt: (optimized: string) => void;
  onRetry: (original: string) => void;
  onDiscard: () => void;
}

/**
 * Shared prompt-optimize comparison panel (steward task 4.7, spec 5.3.1-4,
 * generalized to every composer): original vs optimized side by side with
 * adopt / re-optimize / discard. The draft is never replaced without an
 * explicit adopt — compare first.
 */
export function OptimizeCompareBar({
  original,
  optimized,
  busy,
  onAdopt,
  onRetry,
  onDiscard,
}: OptimizeCompareBarProps): ReactNode {
  const { t } = useTranslation();

  return (
    <div
      role="dialog"
      aria-label={t("composer.optimizeCompareLabel")}
      className="mb-2 rounded border border-ink-muted/30 bg-surface-raised p-2 text-xs text-ink"
    >
      <div className="grid grid-cols-2 gap-2">
        <div>
          <p className="mb-1 font-medium text-ink-muted">{t("composer.optimizeOriginal")}</p>
          <p className="max-h-32 overflow-y-auto whitespace-pre-wrap">{original}</p>
        </div>
        <div>
          <p className="mb-1 font-medium text-ink-accent">{t("composer.optimizeOptimized")}</p>
          <p className="max-h-32 overflow-y-auto whitespace-pre-wrap">{optimized}</p>
        </div>
      </div>
      <div className="mt-2 flex gap-2">
        <button
          type="button"
          className="rounded bg-ink-accent px-2 py-0.5 text-surface disabled:opacity-50"
          onClick={() => onAdopt(optimized)}
        >
          {t("composer.optimizeAdopt")}
        </button>
        <button
          type="button"
          className="rounded border border-ink-muted/40 px-2 py-0.5 text-ink-muted hover:bg-surface disabled:opacity-50"
          disabled={busy}
          onClick={() => onRetry(original)}
        >
          {t("composer.optimizeRetry")}
        </button>
        <button
          type="button"
          className="rounded border border-ink-muted/40 px-2 py-0.5 text-ink-muted hover:bg-surface"
          onClick={onDiscard}
        >
          {t("composer.optimizeDiscard")}
        </button>
      </div>
    </div>
  );
}
