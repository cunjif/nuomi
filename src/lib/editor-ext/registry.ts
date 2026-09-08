/**
 * Editor extension registry (Cordis-mirror): open registration via
 * registerEditorExtension() — aligned with "一切皆插件". Just like
 * src/lib/commands/registry.ts, this is an open array so future
 * Rust-plugin–delivered extensions can be hydrated at startup by calling
 * registerEditorExtension() per entry fetched over IPC — nothing else
 * changes. Builtins self-activate through ensureEditorExtensionsActivated().
 */
import { useSyncExternalStore } from "react";
import type * as Monaco from "monaco-editor";
import type {
  EditorExtContext,
  EditorExtension,
  EditorOverlay,
  EditorPreviewComponent,
  EditorPreviewMode,
  EditorToolbarAction,
  EditorWysiwygComponent,
  SymbolProvider,
} from "./types";
import { registerCommand } from "../commands/registry";

/** localStorage prefix for the per-extension enable toggle (manager panel). */
const ENABLED_PREFIX = "nuomi.editorExt.enabled.";

interface QueuedMonacoProvider {
  extId: string;
  language: string;
  kind: "hover" | "definition" | "reference" | "formatting";
  // Monaco provider interfaces are structural; kept loose here because the
  // queue is flushed against the real Monaco namespace only.
  provider: unknown;
}

interface QueuedEditorReady {
  extId: string;
  cb: (monaco: typeof Monaco, editor: Monaco.editor.IStandaloneCodeEditor) => void;
  /**
   * Editors this callback was already bound to. Per-instance (not global)
   * because EditorArea remounts MonacoTab per open file (key={activeFile}),
   * i.e. every file switch creates a NEW editor that needs its own
   * addCommand/addAction bindings. Weak so disposed instances can be GC'd.
   */
  bound: WeakSet<Monaco.editor.IStandaloneCodeEditor>;
}

interface Contributed<T> {
  extId: string;
  item: T;
}

const extensions: EditorExtension[] = [];
const activated = new Set<string>();
const previews: Contributed<{
  mode: EditorPreviewMode;
  matcher: (path: string) => boolean;
  component: EditorPreviewComponent;
}>[] = [];
const outlines: Contributed<SymbolProvider>[] = [];
const wysiwygEditors: Contributed<{
  matcher: (path: string) => boolean;
  component: EditorWysiwygComponent;
}>[] = [];
const toolbarActions: Contributed<EditorToolbarAction>[] = [];
const overlays: Contributed<EditorOverlay>[] = [];
const capabilities = new Set<string>();
const monacoQueue: QueuedMonacoProvider[] = [];
const readyQueue: QueuedEditorReady[] = [];
/** How many queue entries have been flushed (dedupe across editor remounts). */
let flushedCount = 0;

// --- change notification (useSyncExternalStore-friendly) -------------------

const listeners = new Set<() => void>();
let version = 0;

function notify(): void {
  version += 1;
  listeners.forEach((l) => l());
}

export function subscribeEditorExtChanges(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function getEditorExtVersion(): number {
  return version;
}

/** React hook: re-renders whenever registrations or enable toggles change. */
export function useEditorExtVersion(): number {
  return useSyncExternalStore(subscribeEditorExtChanges, getEditorExtVersion, getEditorExtVersion);
}

// --- enable state -----------------------------------------------------------

export function isEditorExtensionEnabled(id: string): boolean {
  try {
    const raw = localStorage.getItem(ENABLED_PREFIX + id);
    if (raw === "0") return false;
  } catch {
    // Storage unavailable — treat as enabled (default state).
  }
  return true;
}

export function setEditorExtensionEnabled(id: string, enabled: boolean): void {
  try {
    localStorage.setItem(ENABLED_PREFIX + id, enabled ? "1" : "0");
  } catch {
    // Storage unavailable — the toggle stays in-memory for this session.
  }
  notify();
}

// --- registration -----------------------------------------------------------

/** Register an extension; a later registration with the same id wins. */
export function registerEditorExtension(ext: EditorExtension): void {
  const existing = extensions.findIndex((e) => e.id === ext.id);
  if (existing >= 0) extensions.splice(existing, 1);
  extensions.push(ext);
  activated.delete(ext.id);
  notify();
}

export function getRegisteredEditorExtensions(): readonly EditorExtension[] {
  return extensions;
}

function isExtEnabled(extId: string): boolean {
  return isEditorExtensionEnabled(extId);
}

/**
 * Run contribute() for every registered extension that is enabled and not
 * yet activated. Idempotent; previews/outlines/etc. contributed by a
 * disabled extension are filtered at read time (toggling re-renders
 * consumers via the version store — no re-contribute needed).
 */
export function ensureEditorExtensionsActivated(
  monaco: typeof Monaco | null = null,
  editor: Monaco.editor.IStandaloneCodeEditor | null = null,
): void {
  for (const ext of extensions) {
    if (activated.has(ext.id) || !isExtEnabled(ext.id)) continue;
    activated.add(ext.id);
    ext.contribute(makeContext(ext.id, monaco, editor));
  }
  notify();
}

function makeContext(extId: string, monaco: typeof Monaco | null, editor: Monaco.editor.IStandaloneCodeEditor | null): EditorExtContext {
  return {
    monaco,
    editor,
    registerHoverProvider(language, provider) {
      monacoQueue.push({ extId, language, kind: "hover", provider });
    },
    registerDefinitionProvider(language, provider) {
      monacoQueue.push({ extId, language, kind: "definition", provider });
    },
    registerReferenceProvider(language, provider) {
      monacoQueue.push({ extId, language, kind: "reference", provider });
    },
    registerDocumentFormattingEditProvider(language, provider) {
      monacoQueue.push({ extId, language, kind: "formatting", provider });
    },
    registerEditorReady(cb) {
      readyQueue.push({ extId, cb, bound: new WeakSet() });
    },
    registerPreview(options) {
      previews.push({ extId, item: options });
    },
    registerWysiwygEditor(options) {
      wysiwygEditors.push({ extId, item: options });
    },
    registerOutline(provider) {
      outlines.push({ extId, item: provider });
    },
    registerToolbarAction(action) {
      toolbarActions.push({ extId, item: action });
    },
    registerOverlay(overlay) {
      overlays.push({ extId, item: overlay });
    },
    registerCommand(command) {
      registerCommand(command);
    },
    reportCapability(capability) {
      capabilities.add(`${extId}:${capability}`);
    },
  };
}

/**
 * Flush queued Monaco providers into the namespace of a freshly mounted
 * editor. Only entries not yet flushed are registered, so editor tab
 * remounts (Monaco kernel + provider registry persist across them) never
 * produce duplicate providers, while contributions arriving after a
 * toggle-on still get attached. Monaco itself never binds Alt+H/Alt+E, so
 * nothing to unregister here — the shell owns those chords at window
 * capture level (不可覆盖).
 */
export function attachMonacoProviders(
  monaco: typeof Monaco,
  editor: Monaco.editor.IStandaloneCodeEditor | null,
): void {
  const pending = monacoQueue.slice(flushedCount);
  for (const q of pending) {
    if (!isExtEnabled(q.extId)) continue;
    if (q.kind === "hover") monaco.languages.registerHoverProvider(q.language, q.provider as Monaco.languages.HoverProvider);
    else if (q.kind === "definition") monaco.languages.registerDefinitionProvider(q.language, q.provider as Monaco.languages.DefinitionProvider);
    else if (q.kind === "reference") monaco.languages.registerReferenceProvider(q.language, q.provider as Monaco.languages.ReferenceProvider);
    else monaco.languages.registerDocumentFormattingEditProvider(q.language, q.provider as Monaco.languages.DocumentFormattingEditProvider);
  }
  flushedCount = monacoQueue.length;
  // Editor-ready callbacks bind per instance (F12/Alt+F1 live on the
  // editor, not on the monaco namespace), so re-run them for every new
  // editor while skipping instances already bound.
  for (const r of readyQueue) {
    if (!isExtEnabled(r.extId)) continue;
    if (editor !== null && !r.bound.has(editor)) {
      r.bound.add(editor);
      r.cb(monaco, editor);
    }
  }
}

// --- read side (enabled-filtered) ------------------------------------------

export interface ActivePreview {
  extId: string;
  mode: EditorPreviewMode;
  component: EditorPreviewComponent;
}

export function findPreviewForPath(path: string): ActivePreview | null {
  for (const p of previews) {
    if (!isExtEnabled(p.extId) || !p.item.matcher(path)) continue;
    return { extId: p.extId, mode: p.item.mode, component: p.item.component };
  }
  return null;
}

export function getOutlineProviders(): SymbolProvider[] {
  return outlines.filter((o) => isExtEnabled(o.extId)).map((o) => o.item);
}

export interface ActiveWysiwygEditor {
  extId: string;
  component: EditorWysiwygComponent;
}

/** First enabled WYSIWYG editor whose matcher accepts the path, if any. */
export function findWysiwygEditorForPath(path: string): ActiveWysiwygEditor | null {
  for (const w of wysiwygEditors) {
    if (!isExtEnabled(w.extId) || !w.item.matcher(path)) continue;
    return { extId: w.extId, component: w.item.component };
  }
  return null;
}

export function getToolbarActions(): Array<{ extId: string; action: EditorToolbarAction }> {
  return toolbarActions.filter((t) => isExtEnabled(t.extId)).map((t) => ({ extId: t.extId, action: t.item }));
}

export function getOverlays(): Array<{ extId: string; overlay: EditorOverlay }> {
  return overlays.filter((o) => isExtEnabled(o.extId)).map((o) => ({ extId: o.extId, overlay: o.item }));
}

export function getEditorExtensionCapabilities(): string[] {
  return [...capabilities];
}

/** Test-only: wipe all registrations/queues/activation state. */
export function resetEditorExtensionsForTest(): void {
  extensions.length = 0;
  activated.clear();
  previews.length = 0;
  outlines.length = 0;
  wysiwygEditors.length = 0;
  toolbarActions.length = 0;
  overlays.length = 0;
  capabilities.clear();
  monacoQueue.length = 0;
  readyQueue.length = 0;
  flushedCount = 0;
  notify();
}
