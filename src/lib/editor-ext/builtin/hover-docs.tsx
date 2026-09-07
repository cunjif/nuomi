/**
 * Builtin hover-docs extension: Monaco hover float + per-file:line annotation
 * panel.
 *
 * - Hover (Code Definition / Code Doc): when the hovered word matches an
 *   outline symbol (from the SymbolProvider registry), the float shows the
 *   symbol signature — kind, definition line and a context excerpt — plus
 *   any user annotation on that line. LSP INTEGRATION POINT: when a real
 *   language server arrives, register a higher-priority hover/definition
 *   provider through the same EditorExtContext (registerHoverProvider /
 *   registerDefinitionProvider) — this regex fallback stays as offline doc.
 * - Annotations (Code Annotation): free-form notes stored in localStorage
 *   keyed by file path and line. Open via the toolbar button, the editor
 *   context-menu action or Alt+F1 (editor.addCommand; Monaco's own
 *   keybinding system does not claim Alt+F1 by default).
 */
import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { create } from "zustand";
import { useTranslation } from "react-i18next";
import type * as Monaco from "monaco-editor";
import type { EditorExtContext } from "../types";
import { getOutlineProviders } from "../registry";
import { i18n } from "../../../i18n";
import { useUiStore } from "../../../lib/store/uiStore";

export const EXT_ID = "builtin.hover-docs";

const ANNOTATIONS_KEY = "nuomi.editor.annotations";

interface AnnotationEntry {
  line: number;
  text: string;
}

type AnnotationMap = Record<string, AnnotationEntry[]>;

function readAllAnnotations(): AnnotationMap {
  try {
    const raw = localStorage.getItem(ANNOTATIONS_KEY);
    if (raw === null) return {};
    const parsed: unknown = JSON.parse(raw);
    if (parsed !== null && typeof parsed === "object") return parsed as AnnotationMap;
  } catch {
    // Corrupted store behaves like an empty one.
  }
  return {};
}

function writeAllAnnotations(map: AnnotationMap): void {
  try {
    localStorage.setItem(ANNOTATIONS_KEY, JSON.stringify(map));
  } catch {
    // Storage unavailable — annotations stay in-memory for this session.
  }
}

interface AnnotationStoreState {
  open: boolean;
  /** Path whose annotations the panel is showing (mirrors uiStore.activeFile). */
  path: string | null;
  entries: AnnotationEntry[];
  setOpen: (open: boolean) => void;
  syncPath: (path: string | null) => void;
  add: (line: number, text: string) => void;
  remove: (index: number) => void;
}

export const useAnnotationStore = create<AnnotationStoreState>((set) => ({
  open: false,
  path: null,
  entries: [],
  setOpen: (open) => set({ open }),
  syncPath: (path) =>
    set((s) => {
      if (s.path === path) return s;
      return { path, entries: path === null ? [] : (readAllAnnotations()[path] ?? []) };
    }),
  add: (line, text) =>
    set((s) => {
      if (s.path === null) return s;
      const entries = [...s.entries, { line, text }];
      const all = readAllAnnotations();
      all[s.path] = entries;
      writeAllAnnotations(all);
      return { entries };
    }),
  remove: (index) =>
    set((s) => {
      if (s.path === null) return s;
      const entries = s.entries.filter((_, i) => i !== index);
      const all = readAllAnnotations();
      all[s.path] = entries;
      writeAllAnnotations(all);
      return { entries };
    }),
}));

// --- Monaco hover provider --------------------------------------------------

function activeFilePath(): string {
  return useUiStore.getState().activeFile ?? "";
}

const hoverProvider: Monaco.languages.HoverProvider = {
  provideHover(model: Monaco.editor.ITextModel, position: Monaco.Position): Monaco.languages.Hover | null {
    const word = model.getWordAtPosition(position);
    if (word === null) return null;
    const language = model.getLanguageId();
    const symbols = getOutlineProviders()
      .filter((p) => p.languages.includes("*") || p.languages.includes(language))
      .flatMap((p) => p.provideSymbols({ path: activeFilePath(), language, content: model.getValue() }));
    const symbol = symbols.find((s) => s.name === word.word);

    const contents: Monaco.IMarkdownString[] = [];
    if (symbol !== undefined) {
      const defLine = symbol.range.start.line + 1;
      const excerpt = model.getLineContent(defLine).trim();
      contents.push({
        value: `**${i18n.t(`editor.hover.kind_${symbol.kind}`)}** \`${symbol.name}\` · ${i18n.t("editor.hover.definedAt", { line: defLine })}`,
      });
      contents.push({ value: `\`\`\`${language}\n${excerpt}\n\`\`\`` });
    }
    const annotation = readAllAnnotations()[activeFilePath()]?.find((a) => a.line === position.lineNumber);
    if (annotation !== undefined) {
      contents.push({ value: `📌 ${i18n.t("editor.hover.annotation")}: ${annotation.text}` });
    }
    return contents.length > 0 ? { contents } : null;
  },
};

// --- toolbar button + floating panel ----------------------------------------

function AnnotationToolbarButton(): ReactNode {
  const { t } = useTranslation();
  const open = useAnnotationStore((s) => s.open);
  const setOpen = useAnnotationStore((s) => s.setOpen);
  return (
    <button
      type="button"
      onClick={() => setOpen(!open)}
      aria-pressed={open}
      className="rounded border border-ink-muted/40 px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
    >
      📌 {t("editor.annotations")}
    </button>
  );
}

function AnnotationPanel(): ReactNode {
  const { t } = useTranslation();
  const open = useAnnotationStore((s) => s.open);
  const entries = useAnnotationStore((s) => s.entries);
  const add = useAnnotationStore((s) => s.add);
  const remove = useAnnotationStore((s) => s.remove);
  const setOpen = useAnnotationStore((s) => s.setOpen);
  const syncPath = useAnnotationStore((s) => s.syncPath);
  const activeFile = useUiStore((s) => s.activeFile);
  const [lineText, setLineText] = useState("");
  const [text, setText] = useState("");

  useEffect(() => {
    syncPath(activeFile);
  }, [activeFile, syncPath]);

  if (!open || activeFile === null) return null;
  const line = Number.parseInt(lineText, 10);

  return (
    <div
      role="dialog"
      aria-label={t("editor.annotationPanelTitle")}
      className="absolute bottom-4 right-4 z-20 flex max-h-96 w-80 flex-col gap-2 rounded border border-ink-muted/40 bg-surface-raised p-3 shadow-lg"
    >
      <div className="flex items-center justify-between">
        <h3 className="text-xs font-semibold text-ink">{t("editor.annotationPanelTitle")}</h3>
        <button
          type="button"
          onClick={() => setOpen(false)}
          aria-label={t("common.close")}
          className="px-1 text-xs text-ink-muted hover:text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
        >
          ×
        </button>
      </div>
      <p className="truncate font-mono text-[10px] text-ink-muted">{activeFile}</p>
      <ul className="min-h-0 flex-1 overflow-y-auto">
        {entries.length === 0 && <li className="text-xs text-ink-muted">{t("editor.annotationEmpty")}</li>}
        {entries.map((entry, i) => (
          <li key={`${entry.line}-${i}`} className="flex items-start gap-2 py-0.5 text-xs">
            <span className="shrink-0 font-mono text-ink-muted">L{entry.line}</span>
            <span className="min-w-0 flex-1 text-ink">{entry.text}</span>
            <button
              type="button"
              onClick={() => remove(i)}
              aria-label={`${t("common.delete")} L${entry.line}`}
              className="shrink-0 text-ink-muted hover:text-state-danger focus-visible:ring-2 focus-visible:ring-ink-accent"
            >
              ×
            </button>
          </li>
        ))}
      </ul>
      <form
        className="flex flex-col gap-1"
        onSubmit={(e) => {
          e.preventDefault();
          if (Number.isNaN(line) || text.trim().length === 0) return;
          add(line, text.trim());
          setText("");
        }}
      >
        <div className="flex gap-1">
          <input
            type="number"
            min={1}
            aria-label={t("editor.annotationLine")}
            placeholder={t("editor.annotationLine")}
            value={lineText}
            onChange={(e) => setLineText(e.target.value)}
            className="w-16 rounded border border-ink-muted/40 bg-surface px-1 py-0.5 text-xs text-ink"
          />
          <input
            type="text"
            aria-label={t("editor.annotationText")}
            placeholder={t("editor.annotationText")}
            value={text}
            onChange={(e) => setText(e.target.value)}
            className="min-w-0 flex-1 rounded border border-ink-muted/40 bg-surface px-1 py-0.5 text-xs text-ink"
          />
        </div>
        <button
          type="submit"
          className="self-start rounded bg-ink-accent px-2 py-0.5 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
          disabled={Number.isNaN(line) || text.trim().length === 0}
        >
          {t("editor.annotationAdd")}
        </button>
      </form>
    </div>
  );
}

// --- extension --------------------------------------------------------------

export const hoverDocsExtension = {
  id: EXT_ID,
  titleI18nKey: "editor.ext.hoverDocs",
  contribute(ctx: EditorExtContext): void {
    for (const language of ["typescript", "javascript", "rust", "python"]) {
      ctx.registerHoverProvider(language, hoverProvider);
    }
    ctx.registerToolbarAction({ id: `${EXT_ID}.panel`, Component: AnnotationToolbarButton });
    ctx.registerOverlay({ id: `${EXT_ID}.panel`, Component: AnnotationPanel });
    ctx.reportCapability("hover.docs");
    ctx.reportCapability("annotations");
    // Runs once a real editor mounts (see registry.attachMonacoProviders):
    // binds Alt+F1 and the context-menu entry that open the annotation panel.
    ctx.registerEditorReady((monaco, editor) => {
      editor.addCommand(monaco.KeyMod.Alt | monaco.KeyCode.F1, () => useAnnotationStore.getState().setOpen(true));
      editor.addAction({
        id: "nuomi.openAnnotations",
        label: i18n.t("editor.annotations"),
        keybindings: [],
        contextMenuGroupId: "navigation",
        run: () => useAnnotationStore.getState().setOpen(true),
      });
    });
  },
} as const;
