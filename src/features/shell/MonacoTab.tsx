import type { ReactNode } from "react";
import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import type * as Monaco from "monaco-editor";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Spinner } from "../../components/ui/Spinner";
import { describeError } from "../../i18n";
import {
  attachMonacoProviders,
  findPreviewForPath,
  findWysiwygEditorForPath,
  getOutlineProviders,
  useEditorExtVersion,
} from "../../lib/editor-ext";
import type { DocumentSymbol, EditorWysiwygApi } from "../../lib/editor-ext";
import { ipc } from "../../lib/ipc/client";
import { useTheme } from "../../lib/store/useTheme";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";
import { consumePendingJump, registerActiveEditor, updateFileIndex } from "../../lib/editor-ext/indexer/workspace-index";
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
  // WYSIWYG imperative surface (outline reveal) for markdown files. State
  // (not a ref) so pending-jump/active-editor effects re-run once the
  // surface is really mounted (the file query resolves asynchronously).
  const [wysiwygApi, setWysiwygApi] = useState<EditorWysiwygApi | null>(null);
  // Latest-save indirection: the Monaco Ctrl+S command registers once at
  // mount but must always invoke the current closure (value/pending state).
  const saveRef = useRef<() => void>(() => {});
  // Pending jump-flash timer, so a tab switch never clears a disposed
  // editor's decorations collection.
  const flashTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [outlineOpen, setOutlineOpen] = useState(true);
  // Full-width WYSIWYG toolbar host: format buttons are portaled here by the
  // wysiwyg editor so the toolbar spans the outline panel too (用户布局).
  const [wysiwygToolbarHost, setWysiwygToolbarHost] = useState<HTMLDivElement | null>(null);
  // Tab unmount: drop the active-editor registration (reveal/find) even if
  // the Monaco instance was torn down without a dispose event, and cancel a
  // still-running jump flash.
  useEffect(
    () => () => {
      registerActiveEditor(null, () => {});
      if (flashTimerRef.current !== null) clearTimeout(flashTimerRef.current);
    },
    [],
  );

  const preview = findPreviewForPath(path);
  // WYSIWYG-capable files (markdown) get a Typora-style editor by default;
  // the toggle falls back to classic Monaco (with its split preview).
  const wysiwyg = findWysiwygEditorForPath(path);
  const [mdSourceMode, setMdSourceMode] = useState(false);
  const wysiwygActive = wysiwyg !== null && !mdSourceMode;
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
      // Keep the workspace code index fresh for definition/reference nav.
      updateFileIndex(path, value);
      toast.success(t("files.saved"));
    },
    onError: (e) => toast.error(describeError(e)),
  });

  const save = (): void => {
    if (!saveMut.isPending && !fileQuery.isLoading) saveMut.mutate();
  };
  saveRef.current = save;

  const onKeyDown = (e: React.KeyboardEvent): void => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      save();
    }
  };

  const jumpToLine = (line: number): void => {
    // WYSIWYG pane: reveal + focus the block containing this 0-based line.
    if (wysiwygActive) {
      wysiwygApi?.revealLine(line - 1);
      return;
    }
    const editor = editorRef.current;
    if (editor === null) return;
    editor.revealLineInCenter(line);
    editor.setPosition({ lineNumber: line, column: 1 });
    editor.focus();
  };

  // Latest-jump indirection: mount-time effects consume a pending jump and
  // must see the current WYSIWYG/Monaco routing.
  const jumpToLineRef = useRef(jumpToLine);
  jumpToLineRef.current = jumpToLine;

  const applyEdit = (next: string): void => {
    setDraft(next);
    markDirty(path, next !== (fileQuery.data ?? ""));
  };

  // WYSIWYG surface (markdown) never mounts Monaco, so it registers itself
  // as the active editor: cross-file jumps land through revealLine. No find
  // action is contributed — Ctrl+F keeps the webview's native behavior.
  useEffect(() => {
    if (!wysiwygActive) return;
    registerActiveEditor(path, (line) => {
      wysiwygApi?.revealLine(line);
    });
    return () => registerActiveEditor(null, () => {});
  }, [path, wysiwygActive, wysiwygApi]);

  // Consume a pending cross-file jump (F12/Alt+F1 from another file) once
  // this tab is mounted. Markdown needs it here — Monaco's onMount never
  // runs in WYSIWYG mode, so the jump used to be dropped and the stale
  // pendingJump fired later when the user switched to 源码.
  useEffect(() => {
    if (!wysiwygActive || wysiwygApi === null) return; // Monaco consumes it in onMount instead.
    const line = consumePendingJump(path);
    if (line !== null) jumpToLineRef.current(line + 1);
  }, [path, wysiwygActive, wysiwygApi]);

  // View toggles + outline + save: rendered on the WYSIWYG format toolbar
  // row (toolbarExtra) or in the source-mode header, never both.
  const controls: ReactNode = (
    <>
      {wysiwyg !== null && (
        <>
          <button
            type="button"
            onClick={() => setMdSourceMode(false)}
            aria-pressed={wysiwygActive}
            className="sketch-btn px-2 py-0.5 text-xs text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("editor.viewWysiwyg")}
          </button>
          <button
            type="button"
            onClick={() => setMdSourceMode(true)}
            aria-pressed={!wysiwygActive}
            className="sketch-btn px-2 py-0.5 text-xs text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
          >
            {t("editor.viewSource")}
          </button>
        </>
      )}
      {symbols.length > 0 && (
        <button
          type="button"
          onClick={() => setOutlineOpen((o) => !o)}
          aria-pressed={outlineOpen}
          className="sketch-btn px-2 py-0.5 text-xs text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
        >
          {t("editor.outline")}
        </button>
      )}
      <button
        type="button"
        onClick={save}
        disabled={saveMut.isPending || fileQuery.isLoading}
        className="sketch-btn px-2 py-0.5 text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      >
        {t("files.save")}
      </button>
    </>
  );

  const editorBody = fileQuery.isError ? (
    <p className="p-3 text-sm text-state-danger">{describeError(fileQuery.error)}</p>
  ) : fileQuery.isLoading ? (
    <div className="p-3">
      <Spinner label={t("files.loadingFile")} />
    </div>
  ) : wysiwygActive ? (
    <wysiwyg.component
      path={path}
      content={value}
      onChange={applyEdit}
      toolbarContainer={wysiwygToolbarHost}
      onReady={setWysiwygApi}
    />
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
          // Landing flash: briefly highlight the target line of a workspace
          // index jump so the eye catches it (VSCode-like).
          const flash = (line1based: number): void => {
            const decorations = editor.createDecorationsCollection([
              {
                range: new monaco.Range(line1based, 1, line1based, 1),
                options: { isWholeLine: true, className: "jump-flash" },
              },
            ]);
            if (flashTimerRef.current !== null) clearTimeout(flashTimerRef.current);
            flashTimerRef.current = setTimeout(() => {
              flashTimerRef.current = null;
              decorations.clear();
            }, 1200);
          };
          // Workspace-index jump target registration + pending goto (F12
          // from another file lands here after this tab mounts) + the
          // global Ctrl+F find-in-file action.
          registerActiveEditor(
            path,
            (line) => {
              editor.revealLineInCenter(line + 1);
              editor.setPosition({ lineNumber: line + 1, column: 1 });
              editor.focus();
              flash(line + 1);
            },
            {
              find: () => {
                void editor.getAction("actions.find")?.run();
                editor.focus();
              },
            },
          );
          // Editor disposed (tab switch / WYSIWYG swap / unmount): drop the
          // registrations so global chords never touch a dead editor.
          editor.onDidDispose(() => {
            if (editorRef.current === editor) {
              editorRef.current = null;
              registerActiveEditor(null, () => {});
            }
            if (flashTimerRef.current !== null) {
              clearTimeout(flashTimerRef.current);
              flashTimerRef.current = null;
            }
          });
          const jumpLine = consumePendingJump(path);
          if (jumpLine !== null) {
            editor.revealLineInCenter(jumpLine + 1);
            editor.setPosition({ lineNumber: jumpLine + 1, column: 1 });
            flash(jumpLine + 1);
          }
          // Native Monaco chord for Ctrl/Cmd+S (the React handler on the
          // section never sees keys Monaco consumes). Routes through the
          // latest-save ref so it always uses current state.
          editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
            saveRef.current();
          });
        }}
        theme={theme === "dark" ? NUOMI_MONACO_DARK : NUOMI_MONACO_LIGHT}
        value={value}
        path={path}
        onChange={(v) => {
          applyEdit(v ?? "");
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

  const showEditor = !wysiwygActive && (preview === null || preview.mode === "split");
  // Files with a WYSIWYG editor never show the rendered preview pane — the
  // WYSIWYG view IS the rendering, and source mode is plain Monaco (需求:
  // 源码不显示渲染).
  const previewNode =
    wysiwyg === null && preview !== null ? <preview.component key={preview.extId} path={path} content={value} /> : null;

  const outlineList = (
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
  );

  return (
    <section
      aria-label={path}
      onKeyDown={onKeyDown}
      className="flex h-full min-h-0 min-w-0 flex-1 flex-col"
    >
      {/* Source-mode header (breadcrumb + controls). WYSIWYG mode drops this
      row — its controls ride on the format toolbar line (Typora-like). */}
      {showEditor && (
        <div className="flex shrink-0 items-center justify-between gap-2 border-b border-ink-muted/30 px-2 py-1">
          {/* Breadcrumb: file name only (用户截图); full path on hover. */}
          <span className="truncate text-xs text-ink" title={path}>
            {path.split(/[\\/]/).pop() ?? path}
          </span>
          <div className="flex shrink-0 items-center gap-1">{controls}</div>
        </div>
      )}
      {/* WYSIWYG toolbar row spans the FULL tab width (format buttons left,
      view toggles + save right; outline panel starts directly below it). */}
      {wysiwygActive && wysiwyg !== null && (
        <div className="flex shrink-0 items-center gap-2 border-b border-ink-muted/30 px-2 py-1">
          <div
            ref={setWysiwygToolbarHost}
            className="flex min-w-0 flex-wrap items-center gap-0.5"
            /* Format buttons are portaled into this host by the wysiwyg
            editor (toolbarContainer contract). */
            data-testid="wysiwyg-toolbar-host"
          />
          <div className="ml-auto flex shrink-0 items-center gap-1">{controls}</div>
        </div>
      )}
      <div className="flex min-h-0 flex-1">
        {/* Absolute-fill: Monaco measures its container; a definite (not
        content-driven) box guarantees the editor viewport never leaks its
        scrollbar into an outer scroll container. */}
        {showEditor && (
          <div className="relative min-h-0 min-w-0 flex-1">
            <div className="absolute inset-0">{editorBody}</div>
          </div>
        )}
        {/* WYSIWYG pane owns its own scrolling (document-style single pane). */}
        {wysiwygActive && <div className="min-h-0 min-w-0 flex-1">{editorBody}</div>}
        {previewNode}
        {showEditor && outlineOpen && symbols.length > 0 && (
          <nav aria-label={t("editor.outline")} className="min-h-0 w-48 shrink-0 overflow-y-auto border-l border-ink-muted/30 p-2">
            {outlineList}
          </nav>
        )}
        {wysiwygActive && outlineOpen && symbols.length > 0 && (
          /* Top-aligned with the content row: the full-width toolbar row
          above already separates it from the tabs (用户布局). */
          <nav aria-label={t("editor.outline")} className="min-h-0 w-48 shrink-0 overflow-y-auto border-l border-ink-muted/30 p-2">
            {outlineList}
          </nav>
        )}
      </div>
      {(showEditor || wysiwygActive) && (
        <div className="flex shrink-0 items-center justify-between gap-2 border-t border-ink-muted/30 px-2 py-0.5 text-[10px] text-ink-muted">
          {/* Status row (用户截图): full path left, language + save hint right. */}
          <span className="min-w-0 truncate font-mono" title={path}>
            {path}
          </span>
          <span className="flex shrink-0 items-center gap-2">
            <span>
              {t("editor.languageLabel")}: <span className="font-mono">{language}</span>
            </span>
            <span>{t("editor.saveHint")}</span>
          </span>
        </div>
      )}
    </section>
  );
}
