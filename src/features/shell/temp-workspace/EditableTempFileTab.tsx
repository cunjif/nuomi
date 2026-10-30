/**
 * Editable temp-file tab (workbench_enhance spec 5.2 / design 2.2.2).
 * Replaces the read-only TempFileTab with a Monaco editor for in-place
 * editing of external files dragged into the temp workspace. Edits sync
 * to the tempFileTabsStore draft + dirty flag; Ctrl+S opens the
 * SaveTargetDialog for an explicit target path (spec 5.2.1-3).
 */
import type { ReactNode } from "react";
import { lazy, Suspense, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useUiStore, type TempFileEntry } from "../../../lib/store/uiStore";
import { languageForPath } from "../editorLanguage";
import { SaveTargetDialog } from "./SaveTargetDialog";

const MonacoEditor = lazy(async () => {
  await import("../monacoSetup");
  return import("@monaco-editor/react");
});

export function EditableTempFileTab({ entry }: { entry: TempFileEntry }): ReactNode {
  const { t } = useTranslation();
  const updateTempFileDraft = useUiStore((s) => s.updateTempFileDraft);
  const closeTempFileTab = useUiStore((s) => s.closeTempFileTab);
  const [saveOpen, setSaveOpen] = useState(false);
  const [confirmCloseOpen, setConfirmCloseOpen] = useState(false);

  const language = languageForPath(entry.name);

  const handleClose = (): void => {
    if (entry.dirty) {
      setConfirmCloseOpen(true);
    } else {
      closeTempFileTab(entry.absPath);
    }
  };

  const handleSave = (): void => {
    setSaveOpen(true);
  };

  // Ctrl/Cmd+S → open save dialog
  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if ((e.ctrlKey || e.metaKey) && e.key === "s") {
        e.preventDefault();
        handleSave();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  return (
    <div data-testid="editable-temp-file-tab" className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center justify-between gap-2 border-b border-ink-muted/30 px-3 py-1.5">
        <div className="flex min-w-0 items-center gap-2">
          <span className="rounded-sm bg-surface-overlay px-1 py-0.5 text-[10px] uppercase text-ink-muted">
            {t("chat.sources.tempFile")}
          </span>
          <span className="truncate text-sm text-ink" title={entry.absPath}>
            {entry.name}
            {entry.dirty && <span className="ml-0.5 text-state-warn">•</span>}
          </span>
        </div>
        <button
          type="button"
          aria-label={t("chat.sources.closeTempFile")}
          onClick={handleClose}
          className="rounded-full p-1 text-ink-muted hover:bg-surface hover:text-ink"
        >
          <svg viewBox="0 0 16 16" className="h-3.5 w-3.5 fill-current">
            <path d="M3.7 2.3L2.3 3.7 6.6 8l-4.3 4.3 1.4 1.4L8 9.4l4.3 4.3 1.4-1.4L9.4 8l4.3-4.3-1.4-1.4L8 6.6 3.7 2.3z" />
          </svg>
        </button>
      </div>
      <div className="min-h-0 flex-1">
        <Suspense fallback={null}>
          <MonacoEditor
            value={entry.draft}
            language={language}
            onChange={(v: string | undefined) => updateTempFileDraft(entry.absPath, v ?? "")}
            options={{ minimap: { enabled: false }, automaticLayout: true }}
          />
        </Suspense>
      </div>
      <SaveTargetDialog
        open={saveOpen}
        absPath={entry.absPath}
        content={entry.draft}
        onClose={() => setSaveOpen(false)}
      />
      {confirmCloseOpen && (
        <ConfirmCloseDialog
          onConfirm={() => {
            setConfirmCloseOpen(false);
            closeTempFileTab(entry.absPath);
          }}
          onCancel={() => setConfirmCloseOpen(false)}
        />
      )}
    </div>
  );
}

function ConfirmCloseDialog({ onConfirm, onCancel }: { onConfirm: () => void; onCancel: () => void }): ReactNode {
  const { t } = useTranslation();
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30" role="dialog">
      <div className="rounded-sm border border-dashed border-ink-muted/40 bg-surface p-4">
        <p className="text-sm text-ink">{t("workbench.tempWorkspace.unsavedHint")}</p>
        <div className="mt-3 flex justify-end gap-2">
          <button type="button" onClick={onCancel} className="text-xs text-ink-muted hover:text-ink">
            {t("common.cancel")}
          </button>
          <button type="button" onClick={onConfirm} className="text-xs text-state-danger hover:underline">
            {t("workbench.tempWorkspace.confirmClose")}
          </button>
        </div>
      </div>
    </div>
  );
}
