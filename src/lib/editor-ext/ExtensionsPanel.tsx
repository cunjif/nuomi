/**
 * Extension manager popover: lists registered editor extensions with id,
 * localized title and an enable toggle (persisted in localStorage via the
 * registry). Deliberately lives in the editor toolbar — the settings view
 * is owned by another milestone and this surface is editor-scoped.
 */
import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useUiStore } from "../store/uiStore";
import {
  ensureEditorExtensionsActivated,
  getRegisteredEditorExtensions,
  isEditorExtensionEnabled,
  setEditorExtensionEnabled,
  useEditorExtVersion,
} from "./index";

export function ExtensionsPanel(): ReactNode {
  const { t } = useTranslation();
  // Version subscription re-renders the list when registrations change.
  useEditorExtVersion();
  const setView = useUiStore((s) => s.setView);
  const [open, setOpen] = useState(false);
  const extensions = getRegisteredEditorExtensions();

  const toggle = (id: string, next: boolean): void => {
    setEditorExtensionEnabled(id, next);
    // Re-enable runs contribute() for extensions skipped at activation time.
    if (next) ensureEditorExtensionsActivated();
  };

  return (
    <div className="relative">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="rounded border border-ink-muted/40 px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
      >
        🧩 {t("editor.extensions")}
      </button>
      {open && (
        <div
          role="dialog"
          aria-label={t("editor.extPanelTitle")}
          className="absolute right-0 top-full z-20 mt-1 w-72 rounded border border-ink-muted/40 bg-surface-raised p-2 shadow-lg"
        >
          <h3 className="mb-1 text-xs font-semibold text-ink">{t("editor.extPanelTitle")}</h3>
          <p className="mb-2 text-[10px] text-ink-muted">{t("editor.extPanelHint")}</p>
          <button
            type="button"
            onClick={() => {
              setOpen(false);
              // The Extensions Center owns install/uninstall + kernel plugins.
              setView("plugins");
            }}
            className="mb-2 w-full rounded border border-ink-accent/40 px-2 py-1 text-[11px] text-ink-accent hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("plugins.title")} →
          </button>
          {extensions.length === 0 && <p className="text-xs text-ink-muted">{t("common.empty")}</p>}
          <ul>
            {extensions.map((ext) => {
              const enabled = isEditorExtensionEnabled(ext.id);
              return (
                <li key={ext.id} className="flex items-center gap-2 rounded px-1 py-1 text-xs hover:bg-surface-overlay">
                  <span className="min-w-0 flex-1 truncate text-ink" title={ext.id}>
                    {ext.title ?? (ext.titleI18nKey !== undefined ? t(ext.titleI18nKey) : ext.id)}
                    <span className="ml-1 font-mono text-[10px] text-ink-muted">{ext.id}</span>
                  </span>
                  <button
                    type="button"
                    onClick={() => toggle(ext.id, !enabled)}
                    aria-pressed={enabled}
                    className={`shrink-0 rounded border px-1.5 py-0.5 focus-visible:ring-2 focus-visible:ring-ink-accent ${
                      enabled ? "border-state-ok/50 text-state-ok" : "border-ink-muted/40 text-ink-muted"
                    }`}
                  >
                    {enabled ? t("editor.extEnabled") : t("editor.extDisabled")}
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </div>
  );
}
