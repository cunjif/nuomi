import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useUiStore } from "../../lib/store/uiStore";

export function AgentBindingGuide(): ReactNode {
  const { t } = useTranslation();
  const setView = useUiStore((s) => s.setView);
  const setSettingsInitialTab = useUiStore((s) => s.setSettingsInitialTab);

  const goBind = (): void => {
    setSettingsInitialTab("roles");
    setView("settings");
  };

  return (
    <div className="flex items-center gap-2 rounded-[12px_255px_15px_225px/225px_15px_255px_12px] border border-dashed border-ink-muted/60 bg-surface-raised/40 px-3 py-2">
      <p className="flex-1 text-xs text-ink-muted">{t("git.aiCommit.bindingGuide.hint")}</p>
      <button
        type="button"
        onClick={goBind}
        className="rounded-[12px_255px_15px_225px/225px_15px_255px_12px] bg-ink-accent px-3 py-1 font-note-hand text-xs text-surface hover:opacity-90"
      >
        {t("git.aiCommit.bindingGuide.bind")}
      </button>
    </div>
  );
}
