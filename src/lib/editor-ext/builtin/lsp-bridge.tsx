/**
 * Builtin LSP bridge extension: connects Monaco editor providers to Language
 * Server Protocol backends running in the Rust kernel (ADR-0027).
 *
 * Registers CompletionItemProvider, SignatureHelpProvider, HoverProvider,
 * DefinitionProvider, ReferenceProvider, DocumentSymbolProvider,
 * DocumentFormattingEditProvider for each LSP-supported language. Provider
 * calls go through IPC to the Rust LspManager → LspClient → LS process. When
 * LSP is unavailable (LS not installed, startup failure, timeout), providers
 * return empty/null results and Monaco falls back to the regex+index builtin
 * providers (hover-docs, codebase-index, outline-symbols).
 *
 * ADR-0031 navigation parity:
 * - DocumentSymbolProvider feeds Monaco's Ctrl+Shift+O quick outline (LSP
 *   tree first, regex outline fallback when the LSP result is empty).
 * - A global editor opener maps `file://` locations returned by
 *   Definition/Reference providers onto the workspace-index jump plumbing
 *   (uiStore.openFile + pendingJump), making Ctrl+Click / peek jumps work
 *   across files — Monaco standalone has no built-in workspace opener.
 * - Requests fail fast when the LS is known to be failed/disabled (status
 *   cache fed by the status indicator's poll) instead of waiting out the
 *   timeout.
 *
 * File sync (didOpen/didChange/didClose) is wired through registerEditorReady:
 * each editor instance gets its own sync lifecycle on mount/dispose.
 *
 * Diagnostics (L5): an overlay component listens for `lsp://textDocument/publishDiagnostics`
 * Tauri events and converts them to Monaco markers via `monaco.editor.setModelMarkers`.
 */
import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { create } from "zustand";
import type * as Monaco from "monaco-editor";
import type { EditorExtContext } from "../types";
import { ipc } from "../../../lib/ipc/client";
import type { JsonValue } from "../../ipc/bindings.gen";
import { requestOpenSymbol, updateFileIndex } from "../indexer/workspace-index";
import { useUiStore } from "../../../lib/store/uiStore";
import { toast } from "../../../lib/store/toastStore";
import { i18n } from "../../../i18n";
import { useClickOutside } from "../../../components/ui/useClickOutside";
import { extractSymbols } from "./outline-symbols";

export const EXT_ID = "builtin.lsp-bridge";

const LSP_LANGUAGES = ["typescript", "javascript", "rust", "python", "java", "go", "c", "cpp"];

// Timeout tiers (ADR-0031): completion stays tight (typing latency), hover /
// signature / formatting get a mid budget, cross-file navigation gets the
// largest one (LS cold indexes make first definition lookups slow).
const COMPLETION_TIMEOUT_MS = 500;
const REQUEST_TIMEOUT_MS = 2000;
const NAVIGATION_TIMEOUT_MS = 3000;

// --- status cache (fast-fail when the LS is known dead/disabled) ---------------

const lspStatusCache = new Map<string, string>();

/**
 * Feeds the provider fast-fail cache from the status poll (also imported by
 * tests to stage a known status without a live Tauri runtime).
 */
export function updateLspStatusCache(entries: Record<string, string>): void {
  for (const [language, status] of Object.entries(entries)) {
    lspStatusCache.set(language, status);
  }
}

/** True when the LS is known to be unusable — providers return undefined immediately. */
function lspBlocked(language: string): boolean {
  const status = lspStatusCache.get(language);
  return status === "failed" || status === "disabled";
}

interface LspRange {
  start: { line: number; character: number };
  end: { line: number; character: number };
}

interface LspLocation {
  uri: string;
  range: LspRange;
}

function toFileUri(root: string): string {
  return `file:///${root.replaceAll("\\", "/")}`;
}

export function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T | null> {
  return Promise.race([
    promise.catch(() => null),
    new Promise<null>((resolve) => setTimeout(() => resolve(null), ms)),
  ]);
}

function lspRangeToMonaco(monaco: typeof Monaco, range: LspRange): Monaco.IRange {
  return new monaco.Range(
    range.start.line + 1,
    range.start.character + 1,
    range.end.line + 1,
    range.end.character + 1,
  );
}

function safeRange(raw: unknown): LspRange | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  const start = r.start;
  const end = r.end;
  if (typeof start !== "object" || typeof end !== "object" || start === null || end === null) return null;
  const s = start as Record<string, unknown>;
  const e = end as Record<string, unknown>;
  if (typeof s.line !== "number" || typeof s.character !== "number") return null;
  if (typeof e.line !== "number" || typeof e.character !== "number") return null;
  return { start: { line: s.line, character: s.character }, end: { line: e.line, character: e.character } };
}

function safeLocation(raw: unknown): LspLocation | null {
  if (typeof raw !== "object" || raw === null) return null;
  const loc = raw as Record<string, unknown>;
  if (typeof loc.uri !== "string") return null;
  const range = safeRange(loc.range);
  if (range === null) return null;
  return { uri: loc.uri, range };
}

function safeLocations(raw: unknown): LspLocation[] {
  if (Array.isArray(raw)) {
    const result: LspLocation[] = [];
    for (const item of raw) {
      const loc = safeLocation(item);
      if (loc !== null) result.push(loc);
    }
    return result;
  }
  const single = safeLocation(raw);
  return single !== null ? [single] : [];
}

async function getWorkspaceRoot(): Promise<string> {
  try {
    const ws = await ipc.getWorkspace();
    return ws.root ?? "";
  } catch {
    return "";
  }
}

function makeCompletionProvider(monaco: typeof Monaco, language: string): Monaco.languages.CompletionItemProvider {
  return {
    triggerCharacters: [".", ":", "<", ">", "(", "["],
    async provideCompletionItems(model, position): Promise<Monaco.languages.CompletionList | undefined> {
      const root = await getWorkspaceRoot();
      if (root === "") return undefined;
      const result = await withTimeout(
        ipc.lspCompletion(language, toFileUri(root), model.uri.toString(), position.lineNumber - 1, position.column - 1),
        COMPLETION_TIMEOUT_MS,
      );
      if (result === null) return undefined;
      const items = Array.isArray(result) ? result : (result as Record<string, unknown>)?.items;
      if (!Array.isArray(items)) return undefined;
      const word = model.getWordUntilPosition(position);
      const range = new monaco.Range(
        position.lineNumber,
        word.startColumn,
        position.lineNumber,
        word.endColumn,
      );
      const suggestions: Monaco.languages.CompletionItem[] = [];
      for (const item of items) {
        const it = item as Record<string, unknown>;
        const label = it.label as string | undefined;
        if (label === undefined) continue;
        const kindNum = typeof it.kind === "number" ? it.kind : 0;
        const kind = (kindNum > 0 && kindNum <= 25 ? kindNum : 0) as Monaco.languages.CompletionItemKind;
        suggestions.push({
          label,
          kind,
          detail: typeof it.detail === "string" ? it.detail : undefined,
          documentation: typeof it.documentation === "string" ? it.documentation : undefined,
          insertText: typeof it.insertText === "string" ? it.insertText : label,
          insertTextRules:
            typeof it.insertTextFormat === "number" && it.insertTextFormat === 2
              ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet
              : 0,
          range,
        });
      }
      return { suggestions };
    },
  };
}

function makeSignatureHelpProvider(language: string): Monaco.languages.SignatureHelpProvider {
  return {
    signatureHelpTriggerCharacters: ["(", ","],
    async provideSignatureHelp(model, position): Promise<Monaco.languages.SignatureHelpResult | undefined> {
      if (lspBlocked(language)) return undefined;
      const root = await getWorkspaceRoot();
      if (root === "") return undefined;
      const result = await withTimeout(
        ipc.lspSignatureHelp(language, toFileUri(root), model.uri.toString(), position.lineNumber - 1, position.column - 1),
        REQUEST_TIMEOUT_MS,
      );
      if (result === null || typeof result !== "object") return undefined;
      const sigHelp = result as Record<string, unknown>;
      const signatures = sigHelp.signatures;
      if (!Array.isArray(signatures)) return undefined;
      const monacoSignatures: Monaco.languages.SignatureInformation[] = [];
      for (const sig of signatures) {
        const s = sig as Record<string, unknown>;
        const label = s.label as string | undefined;
        if (label === undefined) continue;
        const params = Array.isArray(s.parameters) ? s.parameters : [];
        const monacoParams: Monaco.languages.ParameterInformation[] = params.map((p) => {
          const pr = p as Record<string, unknown>;
          return {
            label: typeof pr.label === "string" ? pr.label : "",
            documentation: typeof pr.documentation === "string" ? pr.documentation : undefined,
          };
        });
        monacoSignatures.push({
          label,
          parameters: monacoParams,
          documentation: typeof s.documentation === "string" ? s.documentation : undefined,
        });
      }
      return {
        value: {
          signatures: monacoSignatures,
          activeSignature: typeof sigHelp.activeSignature === "number" ? sigHelp.activeSignature : 0,
          activeParameter: typeof sigHelp.activeParameter === "number" ? sigHelp.activeParameter : 0,
        },
        dispose() {},
      };
    },
  };
}

function makeHoverProvider(language: string): Monaco.languages.HoverProvider {
  return {
    async provideHover(model, position): Promise<Monaco.languages.Hover | undefined> {
      if (lspBlocked(language)) return undefined;
      const root = await getWorkspaceRoot();
      if (root === "") return undefined;
      const result = await withTimeout(
        ipc.lspHover(language, toFileUri(root), model.uri.toString(), position.lineNumber - 1, position.column - 1),
        REQUEST_TIMEOUT_MS,
      );
      if (result === null || typeof result !== "object") return undefined;
      const hover = result as Record<string, unknown>;
      const contents = hover.contents;
      if (contents === undefined) return undefined;
      let monacoContents: Monaco.IMarkdownString[];
      if (typeof contents === "string") {
        monacoContents = [{ value: contents }];
      } else if (Array.isArray(contents)) {
        monacoContents = contents.map((c) => {
          if (typeof c === "string") return { value: c };
          const mc = c as Record<string, string>;
          return { value: mc.value ?? "" };
        });
      } else {
        const mc = contents as Record<string, string>;
        monacoContents = [{ value: mc.value ?? "" }];
      }
      let range: Monaco.IRange | undefined;
      const r = safeRange(hover.range);
      if (r !== null) {
        range = {
          startLineNumber: r.start.line + 1,
          startColumn: r.start.character + 1,
          endLineNumber: r.end.line + 1,
          endColumn: r.end.character + 1,
        };
      }
      return { contents: monacoContents, range };
    },
  };
}

export function makeDefinitionProvider(monaco: typeof Monaco, language: string): Monaco.languages.DefinitionProvider {
  return {
    async provideDefinition(model, position): Promise<Monaco.languages.Definition | undefined> {
      if (lspBlocked(language)) return undefined;
      const root = await getWorkspaceRoot();
      if (root === "") return undefined;
      const result = await withTimeout(
        ipc.lspDefinition(language, toFileUri(root), model.uri.toString(), position.lineNumber - 1, position.column - 1),
        NAVIGATION_TIMEOUT_MS,
      );
      if (result === null) return undefined;
      const locations = safeLocations(result);
      if (locations.length === 0) return undefined;
      return locations.map((loc) => ({
        uri: monaco.Uri.parse(loc.uri),
        range: lspRangeToMonaco(monaco, loc.range),
      }));
    },
  };
}

function makeReferenceProvider(monaco: typeof Monaco, language: string): Monaco.languages.ReferenceProvider {
  return {
    async provideReferences(model, position): Promise<Monaco.languages.Location[] | undefined> {
      if (lspBlocked(language)) return undefined;
      const root = await getWorkspaceRoot();
      if (root === "") return undefined;
      const result = await withTimeout(
        ipc.lspReferences(language, toFileUri(root), model.uri.toString(), position.lineNumber - 1, position.column - 1),
        NAVIGATION_TIMEOUT_MS,
      );
      if (result === null) return undefined;
      const locations = safeLocations(result);
      if (locations.length === 0) return undefined;
      return locations.map((loc) => ({
        uri: monaco.Uri.parse(loc.uri),
        range: lspRangeToMonaco(monaco, loc.range),
      }));
    },
  };
}

function makeFormattingProvider(monaco: typeof Monaco, language: string): Monaco.languages.DocumentFormattingEditProvider {
  return {
    async provideDocumentFormattingEdits(model): Promise<Monaco.languages.TextEdit[] | undefined> {
      if (lspBlocked(language)) return undefined;
      const root = await getWorkspaceRoot();
      if (root === "") return undefined;
      const result = await withTimeout(
        ipc.lspFormatting(language, toFileUri(root), model.uri.toString()),
        REQUEST_TIMEOUT_MS,
      );
      if (result === null || !Array.isArray(result)) return undefined;
      const edits: Monaco.languages.TextEdit[] = [];
      for (const item of result) {
        const e = item as Record<string, unknown>;
        const range = safeRange(e.range);
        if (range === null) continue;
        edits.push({
          range: lspRangeToMonaco(monaco, range),
          text: typeof e.newText === "string" ? e.newText : "",
        });
      }
      return edits;
    },
  };
}

// --- document symbols (ADR-0031: Ctrl+Shift+O quick outline) -------------------

/**
 * LSP SymbolKind (1-based) → Monaco SymbolKind (0-based). The enumerations
 * are order-aligned (File=1/0 … TypeParameter=26/25), so the mapping is an
 * offset; out-of-range values degrade to File.
 */
function lspSymbolKindToMonaco(kind: number): Monaco.languages.SymbolKind {
  return (kind >= 1 && kind <= 26 ? kind - 1 : 0) as Monaco.languages.SymbolKind;
}

function lspRangeToIRange(range: LspRange): Monaco.IRange {
  return {
    startLineNumber: range.start.line + 1,
    startColumn: range.start.character + 1,
    endLineNumber: range.end.line + 1,
    endColumn: range.end.character + 1,
  };
}

/**
 * LSP → Monaco document symbols. Accepts both response shapes a language
 * server may send: the hierarchical DocumentSymbol[] (with children) and
 * the flat SymbolInformation[] (with location). Malformed entries are
 * dropped; a SymbolInformation becomes a flat top-level symbol.
 */
export function lspDocumentSymbolsToMonaco(raw: unknown[]): Monaco.languages.DocumentSymbol[] {
  const convert = (item: unknown, depth: number): Monaco.languages.DocumentSymbol | null => {
    if (depth > 16 || typeof item !== "object" || item === null) return null;
    const s = item as Record<string, unknown>;
    if (typeof s.name !== "string") return null;
    if (s.selectionRange !== undefined) {
      const range = safeRange(s.range);
      const sel = safeRange(s.selectionRange);
      if (range === null || sel === null) return null;
      const children: Monaco.languages.DocumentSymbol[] = [];
      if (Array.isArray(s.children)) {
        for (const child of s.children) {
          const c = convert(child, depth + 1);
          if (c !== null) children.push(c);
        }
      }
      return {
        name: s.name,
        detail: typeof s.detail === "string" ? s.detail : "",
        kind: lspSymbolKindToMonaco(typeof s.kind === "number" ? s.kind : 0),
        range: lspRangeToIRange(range),
        selectionRange: lspRangeToIRange(sel),
        tags: [],
        children,
      };
    }
    // SymbolInformation: range lives inside location; containerName ignored
    // (Monaco's DocumentSymbol has no container field — flat top-level).
    const loc = s.location;
    if (typeof loc !== "object" || loc === null) return null;
    const range = safeRange((loc as Record<string, unknown>).range);
    if (range === null) return null;
    return {
      name: s.name,
      detail: "",
      kind: lspSymbolKindToMonaco(typeof s.kind === "number" ? s.kind : 0),
      range: lspRangeToIRange(range),
      selectionRange: lspRangeToIRange(range),
      tags: [],
      children: [],
    };
  };
  const result: Monaco.languages.DocumentSymbol[] = [];
  for (const item of raw) {
    const s = convert(item, 0);
    if (s !== null) result.push(s);
  }
  return result;
}

/** Regex outline fallback so Ctrl+Shift+O never goes silent when the LSP has none. */
function regexSymbolsToMonaco(model: Monaco.editor.ITextModel): Monaco.languages.DocumentSymbol[] {
  const symbols = extractSymbols(model.getLanguageId(), model.getValue());
  return symbols.map((s) => ({
    name: s.name,
    detail: "",
    kind: 0 as Monaco.languages.SymbolKind, // File = neutral glyph for regex guesses
    range: {
      startLineNumber: s.range.start.line + 1,
      startColumn: s.range.start.character + 1,
      endLineNumber: s.range.end.line + 1,
      endColumn: s.range.end.character + 1,
    },
    selectionRange: {
      startLineNumber: s.selectionRange.start.line + 1,
      startColumn: s.selectionRange.start.character + 1,
      endLineNumber: s.selectionRange.end.line + 1,
      endColumn: s.selectionRange.end.character + 1,
    },
    tags: [],
    children: [],
  }));
}

export function makeDocumentSymbolProvider(language: string): Monaco.languages.DocumentSymbolProvider {
  return {
    async provideDocumentSymbols(model): Promise<Monaco.languages.DocumentSymbol[] | undefined> {
      if (lspBlocked(language)) return regexSymbolsToMonaco(model);
      const root = await getWorkspaceRoot();
      if (root === "") return regexSymbolsToMonaco(model);
      const result = await withTimeout(
        ipc.lspDocumentSymbols(language, toFileUri(root), model.uri.toString()),
        NAVIGATION_TIMEOUT_MS,
      );
      if (result === null || !Array.isArray(result)) return regexSymbolsToMonaco(model);
      const mapped = lspDocumentSymbolsToMonaco(result);
      return mapped.length > 0 ? mapped : regexSymbolsToMonaco(model);
    },
  };
}

// --- cross-file opener (ADR-0031: Ctrl+Click / peek jumps across files) --------

/**
 * Maps a `file://` fsPath onto a workspace-relative path, or null when it
 * lies outside `root` (Windows-cases-insensitive prefix match). Shared by
 * the editor opener and the QuickOpen workspace-symbol mode.
 */
export function lspUriToRelativePath(fsPath: string, root: string): string | null {
  if (root === "") return null;
  const normRoot = root.replaceAll("\\", "/").replace(/\/+$/, "");
  const normFile = fsPath.replaceAll("\\", "/");
  if (!normFile.toLowerCase().startsWith(`${normRoot.toLowerCase()}/`)) return null;
  return normFile.slice(normRoot.length + 1);
}

/**
 * Maps a `file://` location emitted by the Definition/Reference providers
 * onto the workspace-index jump plumbing. Monaco standalone has no
 * workspace/file-system opener, so without this a Ctrl+Click on a location
 * in another (not-yet-open) file silently does nothing. Returns false for
 * anything outside the current workspace root so other openers still run.
 */
async function openLspLocation(resource: { scheme: string; fsPath: string }, selection: unknown): Promise<boolean> {
  if (resource.scheme !== "file") return false;
  const root = await getWorkspaceRoot();
  const relative = lspUriToRelativePath(resource.fsPath, root);
  if (relative === null) return false;
  const sel = selection as { startLineNumber?: number; lineNumber?: number } | undefined;
  const line = (typeof sel?.startLineNumber === "number" ? sel.startLineNumber : undefined)
    ?? (typeof sel?.lineNumber === "number" ? sel.lineNumber : undefined)
    ?? 1;
  requestOpenSymbol(relative, line - 1);
  return true;
}

let openerRegistered = false;

/** Registers the global editor opener once per Monaco kernel (idempotent). */
export function ensureLspEditorOpener(monaco: typeof Monaco): void {
  if (openerRegistered) return;
  openerRegistered = true;
  monaco.editor.registerEditorOpener({
    openCodeEditor: (_source, resource, selectionOrPosition) =>
      openLspLocation(resource, selectionOrPosition),
  });
}

// --- rename (ADR-0031 M2: LSP rename with a self-drawn apply pipeline) ----------

/**
 * LSP `WorkspaceEdit` → per-file edits. Accepts both wire shapes servers
 * send: `changes` ({uri: TextEdit[]}) and `documentChanges`
 * ([{textDocument: {uri}, edits}]). Malformed entries are dropped.
 */
export function parseWorkspaceEdit(raw: unknown): Array<{ uri: string; edits: Array<{ range: LspRange; newText: string }> }> {
  if (typeof raw !== "object" || raw === null) return [];
  const we = raw as Record<string, unknown>;
  const result: Array<{ uri: string; edits: Array<{ range: LspRange; newText: string }> }> = [];
  const pushEdits = (uri: unknown, edits: unknown): void => {
    if (typeof uri !== "string" || !Array.isArray(edits)) return;
    const parsed: Array<{ range: LspRange; newText: string }> = [];
    for (const e of edits) {
      if (typeof e !== "object" || e === null) continue;
      const er = e as Record<string, unknown>;
      const range = safeRange(er.range);
      if (range === null) continue;
      parsed.push({ range, newText: typeof er.newText === "string" ? er.newText : "" });
    }
    if (parsed.length > 0) result.push({ uri, edits: parsed });
  };
  if (typeof we.changes === "object" && we.changes !== null) {
    for (const [uri, edits] of Object.entries(we.changes as Record<string, unknown>)) {
      pushEdits(uri, edits);
    }
  }
  if (Array.isArray(we.documentChanges)) {
    for (const dc of we.documentChanges) {
      if (typeof dc !== "object" || dc === null) continue;
      const d = dc as Record<string, unknown>;
      if (d.kind !== undefined && d.kind !== "edit") continue; // create/rename/delete unsupported
      const td = d.textDocument;
      if (typeof td === "object" && td !== null) {
        pushEdits((td as Record<string, unknown>).uri, d.edits);
      }
    }
  }
  return result;
}

/**
 * Applies LSP TextEdits to file content. Edits are applied from the end of
 * the document backwards so earlier ranges never shift. Line/character are
 * 0-based LSP positions; CRLF content works because offsets are computed
 * on the raw string.
 */
export function applyLspTextEdits(
  content: string,
  edits: Array<{ range: LspRange; newText: string }>,
): string {
  const lines = content.split("\n");
  const lineStarts: number[] = [0];
  for (let i = 0; i < lines.length; i += 1) {
    lineStarts.push((lineStarts[i] ?? 0) + (lines[i]?.length ?? 0) + 1);
  }
  const posToOffset = (line: number, character: number): number => {
    const l = Math.min(Math.max(line, 0), lines.length - 1);
    return (lineStarts[l] ?? 0) + Math.min(Math.max(character, 0), lines[l]?.length ?? 0);
  };
  const sorted = [...edits].sort((a, b) => {
    const sa = posToOffset(a.range.start.line, a.range.start.character);
    const sb = posToOffset(b.range.start.line, b.range.start.character);
    return sb - sa || posToOffset(b.range.end.line, b.range.end.character) - posToOffset(a.range.end.line, a.range.end.character);
  });
  let out = content;
  for (const e of sorted) {
    const start = posToOffset(e.range.start.line, e.range.start.character);
    const end = posToOffset(e.range.end.line, e.range.end.character);
    out = out.slice(0, start) + e.newText + out.slice(Math.max(start, end));
  }
  return out;
}

const DID_CHANGE_DEBOUNCE_MS = 200;

function bindFileSync(editor: Monaco.editor.IStandaloneCodeEditor): void {
  const model = editor.getModel();
  if (model === null) return;
  const language = model.getLanguageId();
  if (!LSP_LANGUAGES.includes(language)) return;

  let root = "";
  let disposed = false;
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;

  void getWorkspaceRoot().then((r) => {
    if (disposed || r === "") return;
    root = r;
    const fileUri = toFileUri(root);
    void ipc.lspDidOpen(language, fileUri, model.uri.toString(), language, model.getValue()).catch(() => {});
  });

  editor.onDidChangeModelContent(() => {
    if (root === "") return;
    if (debounceTimer !== undefined) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      debounceTimer = undefined;
      if (disposed || root === "") return;
      void ipc.lspDidChange(language, toFileUri(root), model.uri.toString(), model.getVersionId(), model.getValue()).catch(() => {});
    }, DID_CHANGE_DEBOUNCE_MS);
  });

  editor.onDidDispose(() => {
    disposed = true;
    if (debounceTimer !== undefined) clearTimeout(debounceTimer);
    if (root === "") return;
    void ipc.lspDidClose(language, toFileUri(root), model.uri.toString()).catch(() => {});
  });
}

// --- rename flow (prepare → inline panel → workspace edit apply) ----------------

interface RenameContext {
  modelUri: string;
  language: string;
  root: string;
  line: number;
  character: number;
}

interface RenamePanelState {
  open: boolean;
  oldName: string;
  ctx: RenameContext | null;
  close: () => void;
  begin: (start: RenameContext & { oldName: string }) => void;
}

export const useRenamePanelStore = create<RenamePanelState>((set) => ({
  open: false,
  oldName: "",
  ctx: null,
  close: () => set({ open: false, ctx: null }),
  begin: ({ oldName, modelUri, language, root, line, character }) =>
    set({ open: true, oldName, ctx: { modelUri, language, root, line, character } }),
}));

/** F2 entry: validate with prepareRename, then open the inline rename panel. */
export async function startRename(editor: Monaco.editor.IStandaloneCodeEditor): Promise<void> {
  const model = editor.getModel();
  if (model === null) return;
  const language = model.getLanguageId();
  const position = editor.getPosition();
  if (position === null) return;
  const root = await getWorkspaceRoot();
  if (!LSP_LANGUAGES.includes(language) || root === "" || lspBlocked(language)) {
    toast.error(i18n.t("editor.rename.unavailable"));
    return;
  }
  const prepare = await withTimeout(
    ipc.lspPrepareRename(language, toFileUri(root), model.uri.toString(), position.lineNumber - 1, position.column - 1),
    NAVIGATION_TIMEOUT_MS,
  );
  if (prepare === null || typeof prepare !== "object") {
    toast.error(i18n.t("editor.rename.unavailable"));
    return;
  }
  const p = prepare as Record<string, unknown>;
  const range = safeRange(p.range !== undefined ? p.range : prepare);
  if (range === null) {
    toast.error(i18n.t("editor.rename.unavailable"));
    return;
  }
  const oldName =
    typeof p.placeholder === "string" && p.placeholder !== ""
      ? p.placeholder
      : model.getValueInRange({
          startLineNumber: range.start.line + 1,
          startColumn: range.start.character + 1,
          endLineNumber: range.end.line + 1,
          endColumn: range.end.character + 1,
        });
  useRenamePanelStore.getState().begin({
    oldName,
    modelUri: model.uri.toString(),
    language,
    root,
    line: position.lineNumber - 1,
    character: position.column - 1,
  });
}

/** Applies the LSP WorkspaceEdit: open models first, disk round-trip for the rest. */
async function confirmRename(monaco: typeof Monaco, newName: string): Promise<void> {
  const { ctx, close } = useRenamePanelStore.getState();
  if (ctx === null) return;
  close();
  const { modelUri, language, root, line, character } = ctx;
  const result = await withTimeout(
    ipc.lspRename(language, toFileUri(root), modelUri, line, character, newName),
    NAVIGATION_TIMEOUT_MS * 2,
  );
  const changes = parseWorkspaceEdit(result);
  if (changes.length === 0) {
    toast.error(i18n.t("editor.rename.failed"));
    return;
  }
  let applied = 0;
  const failed: string[] = [];
  for (const file of changes) {
    const uri = monaco.Uri.parse(file.uri);
    const model = monaco.editor.getModel(uri);
    if (model !== null) {
      const edits = [...file.edits]
        .sort((a, b) => {
          const sa = a.range.start.line * 1000 + a.range.start.character;
          const sb = b.range.start.line * 1000 + b.range.start.character;
          return sb - sa;
        })
        .map((e) => ({ range: lspRangeToIRange(e.range), text: e.newText }));
      model.pushEditOperations([], edits, () => []);
      applied += 1;
      continue;
    }
    const relative = lspUriToRelativePath(uri.fsPath, root);
    const workspaceId = useUiStore.getState().activeWorkspaceId;
    if (relative === null || workspaceId === null) {
      failed.push(uri.fsPath);
      continue;
    }
    try {
      const content = await ipc.readFile(relative, workspaceId);
      const next = applyLspTextEdits(content, file.edits);
      await ipc.writeFile(relative, next, workspaceId);
      updateFileIndex(relative, next);
      applied += 1;
    } catch {
      failed.push(relative);
    }
  }
  if (failed.length > 0) {
    toast.error(`${i18n.t("editor.rename.failed")}: ${failed.join(", ")}`);
  } else {
    toast.success(`${i18n.t("editor.rename.applied", { count: applied })} · ${newName}`);
  }
}

/** Inline rename input (VSCode-style floating widget at the editor top). */
function LspRenamePanel(): ReactNode {
  const { t } = useTranslation();
  const open = useRenamePanelStore((s) => s.open);
  const oldName = useRenamePanelStore((s) => s.oldName);
  const close = useRenamePanelStore((s) => s.close);
  const [value, setValue] = useState(oldName);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  useClickOutside([rootRef], { enabled: open, onOutside: close });

  useEffect(() => {
    if (!open) return;
    setValue(oldName);
    requestAnimationFrame(() => {
      inputRef.current?.focus();
      inputRef.current?.select();
    });
  }, [open, oldName]);

  const confirm = async (): Promise<void> => {
    const monaco = await import("monaco-editor");
    const name = value.trim();
    if (name === "" || name === oldName) {
      close();
      return;
    }
    close();
    await confirmRename(monaco, name);
  };

  if (!open) return null;
  return (
    <div
      ref={rootRef}
      role="dialog"
      aria-label={t("editor.rename.menuLabel")}
      className="absolute left-1/2 top-2 z-30 -translate-x-1/2"
    >
      <input
        ref={inputRef}
        data-testid="lsp-rename-input"
        value={value}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            void confirm();
          } else if (e.key === "Escape") {
            e.preventDefault();
            close();
          }
        }}
        className="w-72 rounded border border-ink-muted/40 bg-surface-raised px-2 py-1 font-mono text-xs text-ink shadow-lg outline-none focus-visible:ring-2 focus-visible:ring-ink-accent"
      />
    </div>
  );
}

// --- typeDefinition / implementation / codeAction (ADR-0031 M3) -----------------

export function makeTypeDefinitionProvider(monaco: typeof Monaco, language: string): Monaco.languages.TypeDefinitionProvider {
  return {
    async provideTypeDefinition(model, position): Promise<Monaco.languages.Definition | undefined> {
      if (lspBlocked(language)) return undefined;
      const root = await getWorkspaceRoot();
      if (root === "") return undefined;
      const result = await withTimeout(
        ipc.lspTypeDefinition(language, toFileUri(root), model.uri.toString(), position.lineNumber - 1, position.column - 1),
        NAVIGATION_TIMEOUT_MS,
      );
      if (result === null) return undefined;
      const locations = safeLocations(result);
      if (locations.length === 0) return undefined;
      return locations.map((loc) => ({
        uri: monaco.Uri.parse(loc.uri),
        range: lspRangeToMonaco(monaco, loc.range),
      }));
    },
  };
}

export function makeImplementationProvider(monaco: typeof Monaco, language: string): Monaco.languages.ImplementationProvider {
  return {
    async provideImplementation(model, position): Promise<Monaco.languages.Definition | undefined> {
      if (lspBlocked(language)) return undefined;
      const root = await getWorkspaceRoot();
      if (root === "") return undefined;
      const result = await withTimeout(
        ipc.lspImplementation(language, toFileUri(root), model.uri.toString(), position.lineNumber - 1, position.column - 1),
        NAVIGATION_TIMEOUT_MS,
      );
      if (result === null) return undefined;
      const locations = safeLocations(result);
      if (locations.length === 0) return undefined;
      return locations.map((loc) => ({
        uri: monaco.Uri.parse(loc.uri),
        range: lspRangeToMonaco(monaco, loc.range),
      }));
    },
  };
}

/** Monaco marker severity → LSP diagnostic severity (8/4/2/1 → 1/2/3/4). */
function markerSeverityToLsp(severity: Monaco.MarkerSeverity): number {
  if (severity >= 8) return 1;
  if (severity >= 4) return 2;
  if (severity >= 2) return 3;
  return 4;
}

/** Monaco markers → LSP diagnostics for the codeAction request context. */
function markersToLspDiagnostics(markers: Monaco.editor.IMarker[]): JsonValue[] {
  return markers.map((m) => ({
    range: {
      start: { line: m.startLineNumber - 1, character: m.startColumn - 1 },
      end: { line: m.endLineNumber - 1, character: m.endColumn - 1 },
    },
    message: m.message,
    severity: markerSeverityToLsp(m.severity),
    ...(m.source !== undefined ? { source: m.source } : {}),
    ...(m.code !== undefined ? { code: String(m.code) } : {}),
  }));
}

/**
 * LSP CodeAction[] → Monaco CodeAction[]. Only edit-bearing actions map
 * (command-only actions are skipped — LSP command execution is out of
 * scope); the WorkspaceEdit uses the same dual-shape parser as rename. The
 * converted edits keep every file: Monaco applies open models natively, and
 * quick fixes are single-file in practice.
 */
export function lspCodeActionsToMonaco(
  raw: unknown[],
  uriFactory: (uri: string) => Monaco.Uri,
): Monaco.languages.CodeAction[] {
  const result: Monaco.languages.CodeAction[] = [];
  for (const item of raw) {
    if (typeof item !== "object" || item === null) continue;
    const ca = item as Record<string, unknown>;
    if (typeof ca.title !== "string") continue;
    if (typeof ca.command === "string" || typeof ca.command === "object") continue; // command-only
    if (ca.edit === undefined || typeof ca.edit !== "object" || ca.edit === null) continue;
    const files = parseWorkspaceEdit(ca.edit);
    if (files.length === 0) continue;
    const edits: Monaco.languages.IWorkspaceTextEdit[] = [];
    for (const file of files) {
      for (const e of file.edits) {
        edits.push({
          resource: uriFactory(file.uri),
          // versionId omitted: the standalone bulk edit service only checks
          // it when a number is provided — omitting avoids stale-model errors.
          versionId: undefined,
          textEdit: { range: lspRangeToIRange(e.range), text: e.newText },
        });
      }
    }
    result.push({
      title: ca.title,
      kind: typeof ca.kind === "string" ? ca.kind : undefined,
      diagnostics: [],
      edit: { edits },
    });
  }
  return result;
}

export function makeCodeActionProvider(monaco: typeof Monaco, language: string): Monaco.languages.CodeActionProvider {
  return {
    async provideCodeActions(model, range): Promise<Monaco.languages.CodeActionList | undefined> {
      if (lspBlocked(language)) return undefined;
      const root = await getWorkspaceRoot();
      if (root === "") return undefined;
      const markers = monaco.editor.getModelMarkers({ resource: model.uri });
      const diagnostics = markersToLspDiagnostics(markers);
      const result = await withTimeout(
        ipc.lspCodeAction(
          language,
          toFileUri(root),
          model.uri.toString(),
          range.startLineNumber - 1,
          range.startColumn - 1,
          diagnostics,
        ),
        REQUEST_TIMEOUT_MS,
      );
      if (result === null || !Array.isArray(result)) return undefined;
      const actions = lspCodeActionsToMonaco(result, (u) => monaco.Uri.parse(u));
      return actions.length > 0 ? { actions, dispose() {} } : undefined;
    },
  };
}

const DIAGNOSTIC_SEVERITY_ERROR = 8;
const DIAGNOSTIC_SEVERITY_WARNING = 4;
const DIAGNOSTIC_SEVERITY_INFO = 2;
const DIAGNOSTIC_SEVERITY_HINT = 1;

function lspDiagnosticToMarker(
  diag: Record<string, unknown>,
): Monaco.editor.IMarkerData | null {
  const range = safeRange(diag.range);
  if (range === null) return null;
  const severityNum = typeof diag.severity === "number" ? diag.severity : 1;
  const severity =
    severityNum === 1
      ? DIAGNOSTIC_SEVERITY_ERROR
      : severityNum === 2
        ? DIAGNOSTIC_SEVERITY_WARNING
        : severityNum === 3
          ? DIAGNOSTIC_SEVERITY_INFO
          : severityNum === 4
            ? DIAGNOSTIC_SEVERITY_HINT
            : DIAGNOSTIC_SEVERITY_ERROR;
  const message = typeof diag.message === "string" ? diag.message : "";
  if (message.length === 0) return null;
  const source = typeof diag.source === "string" ? diag.source : undefined;
  const code = typeof diag.code === "string" ? diag.code : undefined;
  return {
    severity: severity as Monaco.MarkerSeverity,
    message,
    source,
    code,
    startLineNumber: range.start.line + 1,
    startColumn: range.start.character + 1,
    endLineNumber: range.end.line + 1,
    endColumn: range.end.character + 1,
  };
}

function hasTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function LspDiagnosticsListener(): ReactNode {
  useEffect(() => {
    if (!hasTauriRuntime()) return undefined;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        listen<{
          language: string;
          method: string;
          params: { uri?: string; diagnostics?: unknown[] };
        }>("lsp://textDocument/publishDiagnostics", (e) => {
          const payload = e.payload;
          if (payload === undefined || payload === null) return;
          const uri = payload.params?.uri;
          const diagnostics = payload.params?.diagnostics;
          if (uri === undefined || !Array.isArray(diagnostics)) return;
          void import("monaco-editor").then((monaco) => {
            const model = monaco.editor.getModel(monaco.Uri.parse(uri));
            if (model === null) return;
            const markers: Monaco.editor.IMarkerData[] = [];
            for (const diag of diagnostics) {
              if (typeof diag !== "object" || diag === null) continue;
              const marker = lspDiagnosticToMarker(diag as Record<string, unknown>);
              if (marker !== null) markers.push(marker);
            }
            monaco.editor.setModelMarkers(model, "lsp", markers);
          });
        }),
      )
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
  return null;
}

const STATUS_POLL_MS = 2000;

function classifyEditorStatus(status: string): "not_started" | "initializing" | "ready" | "failed" | "disabled" {
  if (status.startsWith("failed")) return "failed";
  if (status === "ready") return "ready";
  if (status === "initializing") return "initializing";
  if (status === "disabled") return "disabled";
  return "not_started";
}

const STATUS_DOT_BG: Record<string, string> = {
  not_started: "bg-ink-muted/50",
  initializing: "bg-yellow-500",
  ready: "bg-green-500",
  failed: "bg-red-500",
  disabled: "bg-ink-muted/20",
};

function LspStatusIndicator(): ReactNode {
  const [statusMap, setStatusMap] = useState<Record<string, string>>({});
  const [activeLang, setActiveLang] = useState<string | null>(null);

  useEffect(() => {
    if (!hasTauriRuntime()) return undefined;
    let cancelled = false;

    const pollStatuses = async (): Promise<void> => {
      try {
        const statuses = await ipc.lspGetStatuses();
        if (cancelled) return;
        const map: Record<string, string> = {};
        for (const s of statuses) {
          map[s.language] = s.status;
        }
        setStatusMap(map);
        updateLspStatusCache(map);
      } catch {
        // best-effort
      }
    };

    const pollActiveLang = (): void => {
      void import("monaco-editor").then((monaco) => {
        if (cancelled) return;
        const editors = monaco.editor.getEditors();
        const focused = editors.find((e) => e.hasTextFocus()) ?? editors[0];
        const model = focused?.getModel();
        const lang = model?.getLanguageId() ?? null;
        setActiveLang(lang);
      });
    };

    void pollStatuses();
    pollActiveLang();
    const statusTimer = setInterval(() => void pollStatuses(), STATUS_POLL_MS);
    const langTimer = setInterval(pollActiveLang, 1000);
    return () => {
      cancelled = true;
      clearInterval(statusTimer);
      clearInterval(langTimer);
    };
  }, []);

  if (activeLang === null || !LSP_LANGUAGES.includes(activeLang)) return null;
  const raw = statusMap[activeLang] ?? "not_started";
  const kind = classifyEditorStatus(raw);
  const label = kind === "ready" ? "LSP" : kind === "initializing" ? "LSP…" : kind === "failed" ? "LSP!" : "LSP";
  return (
    <div
      className="pointer-events-none absolute bottom-1 right-2 z-10 flex items-center gap-1 rounded bg-surface-overlay/80 px-1.5 py-0.5 text-[10px] text-ink-muted"
      title={`LSP: ${raw}`}
    >
      <span className={`inline-block h-1.5 w-1.5 rounded-full ${STATUS_DOT_BG[kind]}`} />
      {label}
    </div>
  );
}

export const lspBridgeExtension = {
  id: EXT_ID,
  titleI18nKey: "editor.ext.lspBridge",
  contribute(ctx: EditorExtContext): void {
    for (const language of LSP_LANGUAGES) {
      ctx.registerCompletionItemProvider(language, makeCompletionProvider(ctx.monaco ?? ({} as typeof Monaco), language));
      ctx.registerSignatureHelpProvider(language, makeSignatureHelpProvider(language));
      ctx.registerHoverProvider(language, makeHoverProvider(language));
      ctx.registerDefinitionProvider(language, makeDefinitionProvider(ctx.monaco ?? ({} as typeof Monaco), language));
      ctx.registerReferenceProvider(language, makeReferenceProvider(ctx.monaco ?? ({} as typeof Monaco), language));
      ctx.registerDocumentSymbolProvider(language, makeDocumentSymbolProvider(language));
      ctx.registerTypeDefinitionProvider(language, makeTypeDefinitionProvider(ctx.monaco ?? ({} as typeof Monaco), language));
      ctx.registerImplementationProvider(language, makeImplementationProvider(ctx.monaco ?? ({} as typeof Monaco), language));
      ctx.registerCodeActionProvider(language, makeCodeActionProvider(ctx.monaco ?? ({} as typeof Monaco), language));
      ctx.registerDocumentFormattingEditProvider(language, makeFormattingProvider(ctx.monaco ?? ({} as typeof Monaco), language));
    }
    ctx.registerEditorReady((monaco, editor) => {
      ensureLspEditorOpener(monaco);
      bindFileSync(editor);
      const model = editor.getModel();
      if (model !== null && LSP_LANGUAGES.includes(model.getLanguageId())) {
        // F2 overrides Monaco's built-in rename: the standalone bulk edit
        // service throws on edits targeting not-yet-open models, so the
        // WorkspaceEdit application must go through our own pipeline.
        editor.addCommand(monaco.KeyCode.F2, () => {
          void startRename(editor);
        });
        editor.addAction({
          id: "nuomi.lspRenameSymbol",
          label: i18n.t("editor.rename.menuLabel"),
          keybindings: [monaco.KeyCode.F2],
          contextMenuGroupId: "navigation",
          run: () => {
            void startRename(editor);
          },
        });
        // ADR-0031 M3: explicit entries for the type-definition and
        // implementation variants (Monaco binds no default chord for type
        // definition; Ctrl+F12 mirrors VSCode's Go to Implementations).
        // Cross-file jumps land through the registered opener.
        editor.addAction({
          id: "nuomi.goToTypeDefinition",
          label: i18n.t("editor.nav.goToTypeDefinition"),
          contextMenuGroupId: "navigation",
          run: () => {
            void editor.getAction("editor.action.goToTypeDefinition")?.run();
          },
        });
        editor.addAction({
          id: "nuomi.goToImplementation",
          label: i18n.t("editor.nav.goToImplementation"),
          keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.F12],
          contextMenuGroupId: "navigation",
          run: () => {
            void editor.getAction("editor.action.goToImplementation")?.run();
          },
        });
      }
    });
    ctx.registerOverlay({ id: `${EXT_ID}.diagnostics`, Component: LspDiagnosticsListener });
    ctx.registerOverlay({ id: `${EXT_ID}.status`, Component: LspStatusIndicator });
    ctx.registerOverlay({ id: `${EXT_ID}.rename`, Component: LspRenamePanel });
    ctx.reportCapability("lsp.completion");
    ctx.reportCapability("lsp.signatureHelp");
    ctx.reportCapability("lsp.hover");
    ctx.reportCapability("lsp.definition");
    ctx.reportCapability("lsp.references");
    ctx.reportCapability("lsp.documentSymbol");
    ctx.reportCapability("lsp.rename");
    ctx.reportCapability("lsp.typeDefinition");
    ctx.reportCapability("lsp.implementation");
    ctx.reportCapability("lsp.codeAction");
    ctx.reportCapability("lsp.formatting");
    ctx.reportCapability("lsp.diagnostics");
  },
} as const;
