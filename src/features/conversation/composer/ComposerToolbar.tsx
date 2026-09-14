import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../../../components/ui/Icon/Icon";

export interface ComposerToolbarProps {
  onVoiceClick: () => void;
  onFunctionMenuClick: () => void;
  onAttachmentClick: () => void;
  voiceActive?: boolean;
}

/**
 * Toolbar rendered at the bottom-left of the composer textarea.
 * Holds voice input, function list "+", and attachment buttons.
 * Voice and function menu are placeholders — wired up in task groups 4/5.
 */
export function ComposerToolbar({
  onVoiceClick,
  onFunctionMenuClick,
  onAttachmentClick,
  voiceActive = false,
}: ComposerToolbarProps): ReactNode {
  const { t } = useTranslation();

  return (
    <div className="flex items-center gap-0.5">
      <button
        type="button"
        onClick={onVoiceClick}
        className={`flex h-7 w-7 items-center justify-center rounded p-1 text-ink-muted hover:bg-surface-overlay hover:text-ink-accent ${
          voiceActive ? "text-ink-accent" : ""
        }`}
        aria-label={t("composer.toolbar.voice")}
        title={t("composer.toolbar.voice")}
      >
        <Icon name="sparkles" size={16} />
      </button>
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
