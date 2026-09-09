import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../../components/ui/Icon/Icon";
import { useUiStore } from "../../lib/store/uiStore";

/** Open-file tab strip: dirty files show a dot and close via a two-step confirm. */
export function FileTabs(): ReactNode {
  const { t } = useTranslation();
  const openFiles = useUiStore((s) => s.openFiles);
  const activeFile = useUiStore((s) => s.activeFile);
  const dirtyPaths = useUiStore((s) => s.dirtyPaths);
  const setActiveFile = useUiStore((s) => s.setActiveFile);
  const closeFile = useUiStore((s) => s.closeFile);
  /** path awaiting a second click on × (two-step confirm, keyboard friendly) */
  const [confirmingPath, setConfirmingPath] = useState<string | null>(null);

  const requestClose = (path: string): void => {
    if (dirtyPaths[path]) setConfirmingPath(path);
    else closeFile(path);
  };

  return (
    <div role="tablist" aria-label={t("files.treeLabel")} className="flex shrink-0 items-center gap-1 overflow-x-auto border-b border-ink-muted/30 bg-surface px-1 py-1">
      {openFiles.map((path) => {
        const dirty = Boolean(dirtyPaths[path]);
        return (
          <div
            key={path}
            onKeyDown={(e) => {
              if (e.key === "Escape") setConfirmingPath(null);
            }}
            className={`flex shrink-0 items-center rounded text-xs ${
              activeFile === path ? "bg-surface-overlay text-ink" : "text-ink-muted hover:bg-surface-overlay"
            }`}
          >
            <button
              type="button"
              role="tab"
              aria-selected={activeFile === path}
              onClick={() => setActiveFile(path)}
              title={dirty ? `${path} · ${t("tab.unsavedTitle")}` : path}
              className="flex max-w-40 items-center gap-1 truncate px-2 py-1 focus-visible:ring-2 focus-visible:ring-ink-accent"
            >
              <span className="truncate">{path}</span>
              {dirty && <span aria-hidden="true" className="h-1.5 w-1.5 shrink-0 rounded-full bg-state-warn" />}
            </button>
            {confirmingPath === path ? (
              <>
                <button
                  type="button"
                  onClick={() => {
                    setConfirmingPath(null);
                    closeFile(path);
                  }}
                  aria-label={`${t("tab.closeConfirm")} ${path}`}
                  className="px-1 py-1 text-xs text-state-danger hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
                >
                  {t("tab.closeConfirm")}
                </button>
                <button
                  type="button"
                  onClick={() => setConfirmingPath(null)}
                  aria-label={`${t("common.cancel")} ${path}`}
                  className="px-1 py-1 text-xs text-ink-muted hover:text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
                >
                  {t("common.cancel")}
                </button>
              </>
            ) : (
              <button
                type="button"
                onClick={() => requestClose(path)}
                aria-label={`${t("common.close")} ${path}`}
                className="px-1 py-1 text-ink-muted hover:text-state-danger focus-visible:ring-2 focus-visible:ring-ink-accent"
              >
                <Icon name="close" size={12} />
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}
