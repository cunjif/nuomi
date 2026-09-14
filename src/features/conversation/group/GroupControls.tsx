import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

export interface GroupControlsProps {
  onStop: () => void;
  onContinue?: () => void;
  running: boolean;
}

/** Group conversation control bar: stop / continue / add note / export. */
export function GroupControls({ onStop, onContinue, running }: GroupControlsProps): ReactNode {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-1 border-t border-ink-muted/30 px-2 py-1">
      <button
        type="button"
        onClick={onStop}
        disabled={!running}
        className="rounded border border-ink-muted/40 px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay disabled:opacity-50"
      >
        {t("conversation.stop")}
      </button>
      {onContinue && (
        <button
          type="button"
          onClick={onContinue}
          disabled={running}
          className="rounded border border-ink-muted/40 px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay disabled:opacity-50"
        >
          {t("conversation.continue")}
        </button>
      )}
    </div>
  );
}
