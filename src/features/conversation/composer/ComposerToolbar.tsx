import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../../../components/ui/Icon/Icon";

export interface ComposerToolbarProps {
  onFunctionMenuClick: () => void;
  onAttachmentClick: () => void;
}

/**
 * Toolbar rendered at the bottom-left of the composer textarea.
 * Holds function list "+" and attachment buttons.
 * Voice button is rendered separately next to the send button.
 * Function menu is a placeholder — wired up in task group 5.
 */
export function ComposerToolbar({
  onFunctionMenuClick,
  onAttachmentClick,
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
    </div>
  );
}
