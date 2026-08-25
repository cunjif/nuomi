import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useUiStore } from "../../lib/store/uiStore";

/** Open-file tab strip with close buttons. */
export function FileTabs(): ReactNode {
  const { t } = useTranslation();
  const openFiles = useUiStore((s) => s.openFiles);
  const activeFile = useUiStore((s) => s.activeFile);
  const setActiveFile = useUiStore((s) => s.setActiveFile);
  const closeFile = useUiStore((s) => s.closeFile);
  return (
    <div role="tablist" aria-label={t("files.treeLabel")} className="flex shrink-0 items-center gap-1 overflow-x-auto border-b border-ink-muted/30 bg-surface px-1 py-1">
      {openFiles.map((path) => (
        <div
          key={path}
          className={`flex shrink-0 items-center rounded text-xs ${
            activeFile === path ? "bg-surface-overlay text-ink" : "text-ink-muted hover:bg-surface-overlay"
          }`}
        >
          <button
            type="button"
            role="tab"
            aria-selected={activeFile === path}
            onClick={() => setActiveFile(path)}
            title={path}
            className="max-w-40 truncate px-2 py-1 focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {path}
          </button>
          <button
            type="button"
            onClick={() => closeFile(path)}
            aria-label={`${t("common.close")} ${path}`}
            className="px-1 py-1 text-ink-muted hover:text-state-danger focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
