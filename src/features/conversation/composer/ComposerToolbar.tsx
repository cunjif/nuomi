import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../../../components/ui/Icon/Icon";

export interface ComposerToolbarProps {
  onFunctionMenuClick: () => void;
}

/**
 * Toolbar rendered at the bottom-left of the composer textarea.
 * Holds the function list "+" button — the sole entry: attachments ride
 * the menu's three category entries (chat_rich_bind 5.7.1-1a: no
 * duplicate standalone attachment button outside the function menu) and
 * the AI prompt optimize button moved to the input's bottom-right cluster
 * next to voice/send (5.6.1-1a).
 * Voice button is rendered separately next to the send button.
 */
export function ComposerToolbar({
  onFunctionMenuClick,
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
    </div>
  );
}
