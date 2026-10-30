/**
 * Workbench feature toggles (workbench_enhance spec DFX 4.4.2): three
 * independent switches backed by the same app_settings KV mechanism as
 * editor extension toggles. Each capability can be turned off without
 * affecting the other two.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  isEditorExtensionEnabled,
  setEditorExtensionEnabled,
  useEditorExtVersion,
} from "../../lib/editor-ext";

const FLAGS = [
  { id: "workbench.runPanel", labelKey: "workbench.settings.runPanel", descKey: "workbench.settings.runPanelDesc" },
  { id: "workbench.tempWorkspace", labelKey: "workbench.settings.tempWorkspace", descKey: "workbench.settings.tempWorkspaceDesc" },
  { id: "workbench.enhancedHover", labelKey: "workbench.settings.enhancedHover", descKey: "workbench.settings.enhancedHoverDesc" },
] as const;

export function WorkbenchSection(): ReactNode {
  const { t } = useTranslation();
  useEditorExtVersion();

  return (
    <section className="space-y-3">
      <h2 className="text-sm font-semibold text-ink">{t("workbench.settings.heading")}</h2>
      <p className="text-xs text-ink-muted">{t("workbench.settings.hint")}</p>
      <ul className="space-y-2">
        {FLAGS.map((flag) => {
          const enabled = isEditorExtensionEnabled(flag.id);
          return (
            <li
              key={flag.id}
              className="flex items-center justify-between rounded border border-ink-muted/30 bg-surface px-3 py-2"
            >
              <div className="min-w-0 flex-1">
                <div className="text-sm text-ink">{t(flag.labelKey)}</div>
                <div className="text-xs text-ink-muted">{t(flag.descKey)}</div>
              </div>
              <button
                type="button"
                onClick={() => setEditorExtensionEnabled(flag.id, !enabled)}
                aria-pressed={enabled}
                aria-label={t(flag.labelKey)}
                className={`shrink-0 rounded border px-2 py-1 text-xs focus-visible:ring-2 focus-visible:ring-ink-accent ${
                  enabled
                    ? "border-state-ok/50 text-state-ok"
                    : "border-ink-muted/40 text-ink-muted"
                }`}
              >
                {enabled ? t("workbench.settings.on") : t("workbench.settings.off")}
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
