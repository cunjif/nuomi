import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { EmptyState } from "../../components/ui/EmptyState";
import { useUiStore } from "../../lib/store/uiStore";

export function SandboxSetupGuide({ reason }: { reason?: string }): ReactNode {
  const { t } = useTranslation();
  const setView = useUiStore((s) => s.setView);
  const setSettingsInitialTab = useUiStore((s) => s.setSettingsInitialTab);

  const goConfigure = (): void => {
    setSettingsInitialTab("sandbox");
    setView("settings");
  };

  return (
    <EmptyState
      icon="play"
      title={t("sandbox.setupGuide.title")}
      hint={reason ?? t("sandbox.setupGuide.hint")}
      action={
        <button
          type="button"
          onClick={goConfigure}
          className="rounded-[12px_255px_15px_225px/225px_15px_255px_12px] bg-ink-accent px-4 py-1.5 font-note-hand text-xs text-surface hover:opacity-90"
        >
          {t("sandbox.setupGuide.configure")}
        </button>
      }
    />
  );
}
