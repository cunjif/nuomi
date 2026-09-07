import type { ReactNode } from "react";
import { lazy, Suspense, useMemo, useRef, useState } from "react";
import type * as Monaco from "monaco-editor";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Spinner } from "../../components/ui/Spinner";
import { describeError } from "../../i18n";
import {
  attachMonacoProviders,
  findPreviewForPath,
  getOutlineProviders,
  useEditorExtVersion,
} from "../../lib/editor-ext";
import type { DocumentSymbol } from "../../lib/editor-ext";
import { ipc } from "../../lib/ipc/client";
import { useTheme } from "../../lib/store/useTheme";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";
import { NUOMI_MONACO_DARK, NUOMI_MONACO_LIGHT, defineNuomiThemes } from "./monacoThemes";
import { languageForPath } from "./editorLanguage";

// Self-hosted Monaco: bundle the editor locally instead of the default CDN
// loader so the desktop app works fully offline. The setup module is pulled
// in lazily together with the editor chunk.
const MonacoEditor = lazy(async () => {
  await import("./monacoSetup");
  return import("@monaco-editor/react");
});

interface MonacoTabProps {
  path: string;
}

/**
 * One open file: lazy Monaco editor + Ctrl/Cmd+S save with toast feedback,
 * plus the plugin surface (需求 6):
 * - preview-matched files render their preview component instead of (split)
 *   or beside (replace) Monaco — matchers come from the extension registry;
 * - the symbol-outline side panel is fed by registered SymbolProviders and
 *   clicking a symbol jumps to its line via the live editor instance;
 * - on editor mount, queued Monaco providers (hover/definition/formatting)
 *   and editor-ready callbacks (e.g. hover-docs' Alt+F1) are attached.
 *
 * NOTE (不可覆盖 chords): Alt+H / Alt+E are owned by the shell at the
 * window keydown *capture* phase (Shell.tsx) and can never be overridden.
 * Monaco's keybinding system does not register Alt+H/Alt+E by default, and
 * we deliberately add no addCommand for them inside the editor — nothing in
 * this component may shadow the global handler.
 */
export function MonacoTab({ path }: MonacoTabProps): ReactNode {
  const { t } = useTranslation();
  const { theme } = useTheme();
  const qc = useQueryClient();
  const markDirty = useUiStore((s) => s.markDirty);
  // Re-render on registry changes so preview/outline swaps are live.
  useEditorExtVersion();
  const fileQuery = useQuery({ queryKey: ["file", path], queryFn: () => ipc.readFile(path) });
  const [draft, setDraft] = useState<string | null>(null);
  const value = draft ?? fileQuery.data ?? "";
  const editorRef = useRef<Monaco.editor.IStandaloneCodeEditor | null>(null);
  const [outlineOpen, setOutlineOpen] = useState(true);

  const preview = findPreviewForPath(path);
  const language = languageForPath(path);
  const symbols = useMemo<DocumentSymbol[]>(() => {
    if (fileQuery.isLoading || fileQuery.isError) return [];
    const providers = getOutlineProviders().filter(
      (p) => p.languages.includes("*") || p.languages.includes(language),
    );
    return providers.flatMap((p) => p.provideSymbols({ path, language, content: value }));
  }, [fileQuery.isLoading, fileQuery.isError, language, path, value]);

  const saveMut = useMutation({
    mutationFn: () => ipc.writeFile(path, value),
    onSuccess: () => {
      setDraft(null);
      markDirty(path, false);
      void qc.invalidateQueries({ queryKey: ["file", path] });
      toast.success(t("files.saved"));
    },
    onError: (e) => toast.error(describeError(e)),
  });

  const save = (): void => {
    if (!saveMut.isPending && !fileQuery.isLoading) saveMut.mutate();
  };

  const onKeyDown = (e: React.KeyboardEvent): void => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      save();
    }
  };

  const jumpToLine = (line: number): void => {
    const editor = editorRef.current;
    if (editor === null) return;
    editor.revealLineInCenter(line);
    editor.setPosition({ lineNumber: line, column: 1 });
    editor.focus();
  };

  const editorBody = fileQuery.isError ? (
    <p className="p-3 text-sm text-state-danger">{describeError(fileQuery.error)}</p>
  ) : fileQuery.isLoading ? (
    <div className="p-3">
      <Spinner label={t("files.loadingFile")} />
    </div>
  ) : (
    <Suspense
      fallback={
        <div className="p-3">
          <Spinner />
        </div>
      }
    >
      <MonacoEditor
        height="100%"
        defaultLanguage={language}
        beforeMount={defineNuomiThemes}
        onMount={(editor, monaco) => {
          editorRef.current = editor;
          attachMonacoProviders(monaco, editor);
        }}
        theme={theme === "dark" ? NUOMI_MONACO_DARK : NUOMI_MONACO_LIGHT}
        value={value}
        path={path}
        onChange={(v) => {
          const next = v ?? "";
          setDraft(next);
          markDirty(path, next !== (fileQuery.data ?? ""));
        }}
        options={{
          fontSize: 13,
          minimap: { enabled: false },
          tabSize: 2,
          renderWhitespace: "selection",
          smoothScrolling: true,
          scrollBeyondLastLine: false,
        }}
      />
    </Suspense>
  );

  const showEditor = preview === null || preview.mode === "split";
  const previewNode =
    preview !== null ? <preview.component key={preview.extId} path={path} content={value} /> : null;

  return (
    <section aria-label={path} onKeyDown={onKeyDown} className="flex min-w-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center justify-between gap-2 border-b border-ink-muted/30 px-2 py-1">
        <span className="truncate font-mono text-xs text-ink-muted" title={path}>
          {path}
        </span>
        {showEditor && (
          <div className="flex shrink-0 items-center gap-1">
            {symbols.length > 0 && (
              <button
                type="button"
                onClick={() => setOutlineOpen((o) => !o)}
                aria-pressed={outlineOpen}
                className="rounded border border-ink-muted/40 px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
              >
                {t("editor.outline")}
              </button>
            )}
            <button
              type="button"
              onClick={save}
              disabled={saveMut.isPending || fileQuery.isLoading}
              className="rounded border border-ink-muted px-2 py-0.5 text-xs text-ink hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
            >
              {t("files.save")}
            </button>
          </div>
        )}
      </div>
      <div className="flex min-h-0 flex-1">
        {showEditor && <div className="min-w-0 flex-1">{editorBody}</div>}
        {previewNode}
        {showEditor && outlineOpen && symbols.length > 0 && (
          <nav aria-label={t("editor.outline")} className="w-48 shrink-0 overflow-y-auto border-l border-ink-muted/30 p-2">
            <ul className="space-y-0.5">
              {symbols.map((sym, i) => (
                <li key={`${sym.name}-${i}`}>
                  <button
                    type="button"
                    onClick={() => jumpToLine(sym.range.start.line + 1)}
                    className="block w-full truncate rounded text-left text-xs text-ink-muted hover:bg-surface-overlay hover:text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
                    title={`${sym.kind} ${sym.name} · L${sym.range.start.line + 1}`}
                  >
                    <span aria-hidden="true" className="mr-1 text-[10px] uppercase text-ink-muted">
                      {sym.kind}
                    </span>
                    {sym.name}
                  </button>
                </li>
              ))}
            </ul>
          </nav>
        )}
      </div>
      {showEditor && (
        <div className="flex shrink-0 items-center justify-between gap-2 border-t border-ink-muted/30 px-2 py-0.5 text-[10px] text-ink-muted">
          <span>
            {t("editor.languageLabel")}: <span className="font-mono">{language}</span>
          </span>
          <span>{t("editor.saveHint")}</span>
        </div>
      )}
    </section>
  );
}
