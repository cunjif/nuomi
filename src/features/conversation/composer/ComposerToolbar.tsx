import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../../../components/ui/Icon/Icon";

export interface ComposerToolbarProps {
  onFunctionMenuClick: () => void;
  onAttachmentClick: () => void;
  /** Optional AI prompt-optimize toggle; omitted when undefined. */
  onOptimizeClick?: () => void;
  /** True while an optimize call is in flight (spinner state). */
  optimizeRunning?: boolean;
  /** Additional disabled reasons (no draft, pending turn, ...). */
  optimizeDisabled?: boolean;
}

/**
 * Toolbar rendered at the bottom-left of the composer textarea.
 * Holds function list "+", attachment, and (optionally) the AI prompt
 * optimize buttons.
 * Voice button is rendered separately next to the send button.
 * Function menu is a placeholder — wired up in task group 5.
 */
export function ComposerToolbar({
  onFunctionMenuClick,
  onAttachmentClick,
  onOptimizeClick,
  optimizeRunning = false,
  optimizeDisabled = false,
}: ComposerToolbarProps): ReactNode {
  const { t } = useTranslation();

  return (
    <div className="flex items-center gap-0.5">
      <button
        type="button"
        onClick={onFunctionMenuClick}
        className="flex h-7 w-7 items-center justify-center rounded p-1 text-ink-muted hover:bg-surface-overlay hover:text-ink-accent"
        aria-label={t("composer.toolbar.functionMenu")}
        title={t("composer.toolbar.functionMenu")}
      >
        <Icon name="plus" size={16} />
      </button>
      <button
        type="button"
        onClick={onAttachmentClick}
        className="flex h-7 w-7 items-center justify-center rounded p-1 text-ink-muted hover:bg-surface-overlay hover:text-ink-accent"
        aria-label={t("composer.toolbar.attachment")}
        title={t("composer.toolbar.attachment")}
      >
        <Icon name="file" size={16} />
      </button>
      {onOptimizeClick !== undefined && (
        <button
          type="button"
          onClick={onOptimizeClick}
          className="flex h-7 w-7 items-center justify-center rounded p-1 text-ink-muted hover:bg-surface-overlay hover:text-ink-accent disabled:opacity-50"
          aria-label={t("composer.optimize")}
          title={t(optimizeRunning ? "composer.optimizeRunning" : "composer.optimize")}
          disabled={optimizeDisabled || optimizeRunning}
        >
          <Icon
            name={optimizeRunning ? "spinner" : "wand"}
            size={16}
            className={optimizeRunning ? "animate-spin" : undefined}
          />
        </button>
      )}
    </div>
  );
}
