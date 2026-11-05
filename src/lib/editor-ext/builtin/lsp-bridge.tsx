/**
 * Builtin LSP bridge extension: connects Monaco editor providers to Language
 * Server Protocol backends running in the Rust kernel (ADR-0027).
 *
 * Registers CompletionItemProvider, SignatureHelpProvider, HoverProvider,
 * DefinitionProvider, ReferenceProvider, DocumentFormattingEditProvider for
 * each LSP-supported language. Provider calls go through IPC to the Rust
 * LspManager → LspClient → LS process. When LSP is unavailable (LS not
 * installed, startup failure, timeout), providers return empty/null results
 * and Monaco falls back to the regex+index builtin providers (hover-docs,
 * codebase-index, outline-symbols).
 *
 * File sync (didOpen/didChange/didClose) is wired through registerEditorReady:
 * each editor instance gets its own sync lifecycle on mount/dispose.
 *
 * Diagnostics (L5): an overlay component listens for `lsp://textDocument/publishDiagnostics`
 * Tauri events and converts them to Monaco markers via `monaco.editor.setModelMarkers`.
 */
import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import type * as Monaco from "monaco-editor";
import type { EditorExtContext } from "../types";
import { ipc } from "../../../lib/ipc/client";

export const EXT_ID = "builtin.lsp-bridge";

const LSP_LANGUAGES = ["typescript", "javascript", "rust", "python", "java", "go", "c", "cpp"];

const COMPLETION_TIMEOUT_MS = 500;
const REQUEST_TIMEOUT_MS = 2000;

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

function withTimeout<T>(promise: Promise<T>, ms: number): Promise<T | null> {
  return Promise.race([
    promise,
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

function makeDefinitionProvider(monaco: typeof Monaco, language: string): Monaco.languages.DefinitionProvider {
  return {
    async provideDefinition(model, position): Promise<Monaco.languages.Definition | undefined> {
      const root = await getWorkspaceRoot();
      if (root === "") return undefined;
      const result = await withTimeout(
        ipc.lspDefinition(language, toFileUri(root), model.uri.toString(), position.lineNumber - 1, position.column - 1),
        REQUEST_TIMEOUT_MS,
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
      const root = await getWorkspaceRoot();
      if (root === "") return undefined;
      const result = await withTimeout(
        ipc.lspReferences(language, toFileUri(root), model.uri.toString(), position.lineNumber - 1, position.column - 1),
        REQUEST_TIMEOUT_MS,
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
    void ipc.lspDidOpen(language, fileUri, model.uri.toString(), language, model.getValue());
  });

  editor.onDidChangeModelContent(() => {
    if (root === "") return;
    if (debounceTimer !== undefined) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      debounceTimer = undefined;
      if (disposed || root === "") return;
      void ipc.lspDidChange(language, toFileUri(root), model.uri.toString(), model.getVersionId(), model.getValue());
    }, DID_CHANGE_DEBOUNCE_MS);
  });

  editor.onDidDispose(() => {
    disposed = true;
    if (debounceTimer !== undefined) clearTimeout(debounceTimer);
    if (root === "") return;
    void ipc.lspDidClose(language, toFileUri(root), model.uri.toString());
  });
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
      ctx.registerDocumentFormattingEditProvider(language, makeFormattingProvider(ctx.monaco ?? ({} as typeof Monaco), language));
    }
    ctx.registerEditorReady((_monaco, editor) => {
      bindFileSync(editor);
    });
    ctx.registerOverlay({ id: `${EXT_ID}.diagnostics`, Component: LspDiagnosticsListener });
    ctx.registerOverlay({ id: `${EXT_ID}.status`, Component: LspStatusIndicator });
    ctx.reportCapability("lsp.completion");
    ctx.reportCapability("lsp.signatureHelp");
    ctx.reportCapability("lsp.hover");
    ctx.reportCapability("lsp.definition");
    ctx.reportCapability("lsp.references");
    ctx.reportCapability("lsp.formatting");
    ctx.reportCapability("lsp.diagnostics");
  },
} as const;
