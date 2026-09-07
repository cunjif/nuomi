/**
 * Plugin-style editor extension contracts (前端镜像 of the Cordis "一切皆插件"
 * kernel idea in crates/nuomi-core/src/harness). Extensions contribute
 * previews, outlines, Monaco providers, toolbar actions and overlays through
 * an EditorExtContext instead of importing app internals — the same shape a
 * future Rust-plugin–delivered extension will implement (typed manifest
 * hydration over IPC, see registry.ts note).
 */
import type { ReactNode } from "react";
import type * as Monaco from "monaco-editor";
import type { SlashCommand } from "../../lib/commands/registry";

/** Minimal file descriptor handed to symbol/outline providers. */
export interface EditorFileRef {
  path: string;
  /** Monaco language id, e.g. "typescript" | "rust" | "markdown". */
  language: string;
  content: string;
}

/**
 * LSP-shaped DocumentSymbol (subset: name/kind/range/selectionRange).
 * Deliberately mirrors the LSP DocumentSymbol structure so a real
 * tree-sitter or LSP backend can be dropped in as a same-interface plugin
 * without touching consumers.
 */
export interface DocumentSymbol {
  name: string;
  /** Coarse kind string: "function" | "class" | "struct" | "method" | "heading" | … */
  kind: string;
  detail?: string;
  /** 0-based LSP-style positions. */
  range: { start: Position; end: Position };
  selectionRange: { start: Position; end: Position };
}

export interface Position {
  line: number;
  character: number;
}

export interface SymbolProvider {
  /** Stable provider id, e.g. "builtin.regex.symbols". */
  id: string;
  /** Monaco language ids handled; ["*"] matches any language. */
  languages: readonly string[];
  provideSymbols(doc: EditorFileRef): DocumentSymbol[];
}

/** "replace": preview stands in for Monaco; "split": Monaco left + preview right. */
export type EditorPreviewMode = "replace" | "split";

export interface EditorPreviewProps {
  path: string;
  /** Current file text (already loaded via the readFile IPC). */
  content: string;
}

export type EditorPreviewComponent = (props: EditorPreviewProps) => ReactNode;

/** Toolbar button contributed into the editor top bar; self-contained component. */
export interface EditorToolbarAction {
  id: string;
  Component: () => ReactNode;
}

/** Always-mounted panel (floating window) owned by an extension. */
export interface EditorOverlay {
  id: string;
  Component: () => ReactNode;
}

/**
 * Services handed to an extension's contribute(). Monaco-dependent methods
 * queue their providers in the registry; they are flushed into the real
 * Monaco namespace when an editor instance mounts (attachMonacoProviders).
 * In tests / preview-only rendering monaco is null and queues are inert.
 */
export interface EditorExtContext {
  /** Monaco namespace once the self-hosted kernel loaded; null before that. */
  readonly monaco: typeof Monaco | null;
  /** Live editor instance of the active tab; null when no Monaco tab is mounted. */
  readonly editor: Monaco.editor.IStandaloneCodeEditor | null;
  registerHoverProvider(language: string, provider: Monaco.languages.HoverProvider): void;
  registerDefinitionProvider(language: string, provider: Monaco.languages.DefinitionProvider): void;
  registerDocumentFormattingEditProvider(
    language: string,
    provider: Monaco.languages.DocumentFormattingEditProvider,
  ): void;
  /**
   * Callback invoked once a real editor instance mounts (Alt+F1 style
   * editor.addCommand/addAction bindings live here, not in contribute —
   * contribute usually runs before any editor exists).
   */
  registerEditorReady(cb: (monaco: typeof Monaco, editor: Monaco.editor.IStandaloneCodeEditor) => void): void;
  registerPreview(options: {
    mode: EditorPreviewMode;
    matcher: (path: string) => boolean;
    component: EditorPreviewComponent;
  }): void;
  registerOutline(provider: SymbolProvider): void;
  registerToolbarAction(action: EditorToolbarAction): void;
  registerOverlay(overlay: EditorOverlay): void;
  /** Reuses the chat slash-command registry (src/lib/commands). */
  registerCommand(command: SlashCommand): void;
  /** Declare a capability the extension provides (surfaced in the manager panel). */
  reportCapability(capability: string): void;
}

export interface EditorExtension {
  /** Globally unique, e.g. "builtin.preview-markdown". */
  id: string;
  /** i18n key of the human title shown in the extension manager. */
  titleI18nKey: string;
  contribute(ctx: EditorExtContext): void;
}
