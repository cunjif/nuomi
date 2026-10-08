import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

interface QuotePreviewBarProps {
  excerpt: string;
  onCancel: () => void;
}

/** Bar shown above the composer textarea when quoting a message. */
export function QuotePreviewBar({ excerpt, onCancel }: QuotePreviewBarProps): ReactNode {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-2 border-t border-ink-muted/30 bg-surface-overlay/50 px-3 py-1.5 text-xs text-ink-muted">
      <span className="shrink-0 font-medium text-ink-accent">{t("chat.quoting")}</span>
      <span className="min-w-0 flex-1 truncate">{excerpt}</span>
      <button
        type="button"
        onClick={onCancel}
        className="shrink-0 text-ink-muted hover:text-ink"
        aria-label={t("chat.cancelQuote")}
      >
        <svg viewBox="0 0 16 16" className="h-3.5 w-3.5 fill-current">
          <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" strokeWidth="1.5" fill="none" />
        </svg>
      </button>
    </div>
  );
}
