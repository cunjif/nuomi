/**
 * Builtin codebase-index extension (需求重构 2): automatic, in-app workspace
 * code index driving VSCode-style navigation — no Agent, no external MCP.
 *
 * - An invisible overlay kicks off `reindexWorkspace()` whenever the
 *   workspace root appears/changes (the workspace query is shared with the
 *   shell, so it fires once the kernel is ready).
 * - Registers Monaco definition/reference providers backed by the index.
 *   Ctrl+Click / Shift+F12 use Monaco's native peek (all locations are
 *   returned); F12 jumps straight to the single definition or opens the
 *   navigation picker when a symbol has several; Alt+F1 opens the
 *   references panel with line snippets. Cross-file jumps go through
 *   uiStore.openFile + the pending-jump plumbing (with a landing flash).
 */
import type { ReactNode } from "react";
import { useEffect, useRef } from "react";
import { create } from "zustand";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import type * as Monaco from "monaco-editor";
import type { EditorExtContext } from "../types";
import { getOutlineProviders } from "../registry";
import { ipc } from "../../../lib/ipc/client";
import { i18n } from "../../../i18n";
import {
  getWorkspaceIndex,
  reindexWorkspace,
  requestOpenSymbol,
  type ReferenceHit,
} from "../indexer/workspace-index";

export const EXT_ID = "builtin.codebase-index";

const NAV_LANGUAGES = ["typescript", "javascript", "rust", "python", "java", "go", "c", "cpp"];

function wordAt(model: Monaco.editor.ITextModel, position: Monaco.Position): string | null {
  const word = model.getWordAtPosition(position);
  return word === null ? null : word.word;
}

/**
 * Providers only run once the lazy Monaco kernel is loaded, so the Uri
 * constructor comes from a dynamic import of the same chunk (no eager
 * monaco in the main bundle).
 */
async function fileLocation(path: string, range: { startLineNumber: number; endColumn: number }): Promise<Monaco.languages.Location> {
  const { Uri } = await import("monaco-editor");
  return {
    uri: Uri.parse(`file:///${path.replaceAll("\\", "/")}`),
    range: {
      startLineNumber: range.startLineNumber,
      startColumn: 1,
      endLineNumber: range.startLineNumber,
      endColumn: range.endColumn,
    },
  };
}

const definitionProvider: Monaco.languages.DefinitionProvider = {
  async provideDefinition(model, position) {
    const word = wordAt(model, position);
    if (word === null) return null;
    let hits = getWorkspaceIndex().data?.definitions.get(word) ?? [];
    // Index not built yet (or word unindexed): fall back to an on-the-fly
    // outline scan of the open file so in-file navigation still works.
    if (hits.length === 0) {
      const language = model.getLanguageId();
      const local = getOutlineProviders()
        .filter((p) => p.languages.includes("*") || p.languages.includes(language))
        .flatMap((p) => p.provideSymbols({ path: "", language, content: model.getValue() }))
        .find((s) => s.name === word);
      if (local !== undefined) {
        // Same-file fallback: target the open model's own URI (an empty
        // path used to produce `file:///` and Monaco dropped the jump).
        const line = local.range.start.line + 1;
        return {
          uri: model.uri,
          range: { startLineNumber: line, startColumn: 1, endLineNumber: line, endColumn: 1 },
        };
      }
      return null;
    }
    hits = hits.slice();
    // Same-file definitions first (VSCode-like preference).
    const modelPath = model.uri.path.replace(/^\//, "");
    hits.sort((a, b) => Number(b.path === modelPath) - Number(a.path === modelPath));
    return Promise.all(
      hits.map((d) => fileLocation(d.path, { startLineNumber: d.line + 1, endColumn: (d.signature.length || 1) + 1 })),
    );
  },
};

const referenceProvider: Monaco.languages.ReferenceProvider = {
  async provideReferences(model, position) {
    const word = wordAt(model, position);
    if (word === null) return null;
    const hits = getWorkspaceIndex().data?.references.get(word);
    if (hits === undefined || hits.length === 0) return null;
    return Promise.all(hits.map((r) => fileLocation(r.path, { startLineNumber: r.line + 1, endColumn: 1 })));
  },
};

// --- navigation picker / references side panel ---------------------------------

interface NavHit {
  path: string;
  line: number;
  text: string;
}

interface NavPanelState {
  open: boolean;
  word: string;
  hits: NavHit[];
  close: () => void;
  show: (word: string, hits: NavHit[]) => void;
}

export const useNavPanelStore = create<NavPanelState>((set) => ({
  open: false,
  word: "",
  hits: [],
  close: () => set({ open: false }),
  show: (word, hits) => set({ open: true, word, hits }),
}));

function toNavHits(hits: ReferenceHit[]): NavHit[] {
  return hits.map((h) => ({ path: h.path, line: h.line, text: h.text }));
}

function NavPanel(): ReactNode {
  const { t } = useTranslation();
  const open = useNavPanelStore((s) => s.open);
  const word = useNavPanelStore((s) => s.word);
  const hits = useNavPanelStore((s) => s.hits);
  const close = useNavPanelStore((s) => s.close);
  if (!open) return null;
  return (
    <div
      role="dialog"
      aria-label={t("editor.nav.findReferences")}
      className="absolute bottom-4 right-4 z-20 flex max-h-96 w-96 flex-col gap-2 rounded border border-ink-muted/40 bg-surface-raised p-3 shadow-lg"
    >
      <div className="flex items-center justify-between">
        <h3 className="truncate text-xs font-semibold text-ink">
          <span className="font-mono">{word}</span> · {hits.length}
        </h3>
        <button
          type="button"
          onClick={close}
          aria-label={t("common.close")}
          className="px-1 text-xs text-ink-muted hover:text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
        >
          ×
        </button>
      </div>
      <ul className="min-h-0 flex-1 overflow-y-auto">
        {hits.length === 0 && <li className="text-xs text-ink-muted">{t("editor.nav.noReferences")}</li>}
        {hits.map((hit, i) => (
          <li key={`${hit.path}-${hit.line}-${i}`}>
            <button
              type="button"
              onClick={() => {
                close();
                requestOpenSymbol(hit.path, hit.line);
              }}
              className="flex w-full items-baseline gap-2 rounded px-1 py-0.5 text-left text-xs hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
              title={`${hit.path}:${hit.line + 1}`}
            >
              <span className="min-w-0 flex-1 truncate font-mono text-ink">{hit.text !== "" ? hit.text : hit.path}</span>
              <span className="shrink-0 font-mono text-ink-muted">
                {hit.path}:{hit.line + 1}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** Editor command surface: F12 definition jump/picker, Alt+F1 references. */
function bindEditorCommands(monaco: typeof Monaco, editor: Monaco.editor.IStandaloneCodeEditor): void {
  const currentWord = (): string | null => {
    const model = editor.getModel();
    if (model === null) return null;
    const position = editor.getPosition() ?? { lineNumber: 1, column: 1 };
    const word = model.getWordAtPosition(position);
    return word === null ? null : word.word;
  };
  const jumpToDefinition = (): void => {
    const word = currentWord();
    if (word === null) return;
    const defs = getWorkspaceIndex().data?.definitions.get(word) ?? [];
    if (defs.length === 1) {
      const d = defs[0];
      if (d !== undefined) requestOpenSymbol(d.path, d.line);
      return;
    }
    if (defs.length > 1) {
      // Ambiguous symbol: pick from the navigation panel instead of guessing.
      useNavPanelStore
        .getState()
        .show(word, defs.map((d) => ({ path: d.path, line: d.line, text: d.signature })));
    }
  };
  const findReferences = (): void => {
    const word = currentWord();
    if (word === null) return;
    useNavPanelStore.getState().show(word, toNavHits(getWorkspaceIndex().data?.references.get(word) ?? []));
  };
  editor.addCommand(monaco.KeyCode.F12, jumpToDefinition);
  editor.addCommand(monaco.KeyMod.Alt | monaco.KeyCode.F1, findReferences);
  editor.addAction({
    id: "nuomi.gotoDefinition",
    label: i18n.t("editor.nav.gotoDefinition"),
    keybindings: [monaco.KeyCode.F12],
    contextMenuGroupId: "navigation",
    run: jumpToDefinition,
  });
  editor.addAction({
    id: "nuomi.findReferences",
    label: i18n.t("editor.nav.findReferences"),
    keybindings: [monaco.KeyMod.Alt | monaco.KeyCode.F1],
    contextMenuGroupId: "navigation",
    run: findReferences,
  });
}

// --- automatic indexing trigger -------------------------------------------------

/** Invisible overlay: rebuild the index whenever the workspace root changes. */
function AutoIndexer(): ReactNode {
  const workspaceQuery = useQuery({ queryKey: ["workspace"], queryFn: ipc.getWorkspace, staleTime: 30_000 });
  const configured = workspaceQuery.data?.configured ?? false;
  const root = workspaceQuery.data?.root ?? null;
  const lastRootRef = useRef<string | null>(null);
  useEffect(() => {
    if (!configured || root === null) return;
    if (lastRootRef.current === root) return;
    lastRootRef.current = root;
    void reindexWorkspace();
  }, [configured, root]);
  return null;
}

export const codebaseIndexExtension = {
  id: EXT_ID,
  titleI18nKey: "editor.ext.codebaseIndex",
  contribute(ctx: EditorExtContext): void {
    for (const language of NAV_LANGUAGES) {
      ctx.registerDefinitionProvider(language, definitionProvider);
      ctx.registerReferenceProvider(language, referenceProvider);
    }
    ctx.registerOverlay({ id: `${EXT_ID}.auto`, Component: AutoIndexer });
    ctx.registerOverlay({ id: `${EXT_ID}.nav`, Component: NavPanel });
    ctx.registerEditorReady(bindEditorCommands);
    ctx.reportCapability("codebase.index");
    ctx.reportCapability("nav.definition");
    ctx.reportCapability("nav.references");
  },
} as const;
