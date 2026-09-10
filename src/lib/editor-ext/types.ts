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

/**
 * A WYSIWYG editor stands in for Monaco entirely (Typora-style single pane).
 * Unlike previews it owns edits: every DOM change must be round-tripped back
 * to markdown through `onChange` so the tab's draft/save pipeline (Ctrl+S)
 * keeps working unchanged.
 */
export interface EditorWysiwygProps {
  path: string;
  /** Current file text (draft-aware: parent re-renders with our own onChange output). */
  content: string;
  onChange: (next: string) => void;
  /**
   * Imperative API handed to the host once the editor surface is ready;
   * called with `null` when the surface unmounts so the host never keeps a
   * dangling handle to a dead component.
   */
  onReady?: (api: EditorWysiwygApi | null) => void;
  /**
   * Extra controls rendered on the format toolbar row's right end (view
   * toggles / outline / save live on the same line as B/I/H1… per 用户布局).
   */
  toolbarExtra?: ReactNode;
  /**
   * When provided, the format toolbar renders into this container (portal)
   * instead of inline — lets the host span the toolbar across side panels.
   * `undefined` = inline toolbar (default); `null` = host row not mounted yet.
   */
  toolbarContainer?: HTMLElement | null;
}

export type EditorWysiwygComponent = (props: EditorWysiwygProps) => ReactNode;

/**
 * Host-side control surface (outline navigation et al). `line` is 0-based,
 * mirroring the LSP positions used by SymbolProvider.
 */
export interface EditorWysiwygApi {
  revealLine(line: number): void;
}

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
  registerReferenceProvider(language: string, provider: Monaco.languages.ReferenceProvider): void;
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
  /** Register a Typora-style WYSIWYG editor for matching files (wins over previews/Monaco). */
  registerWysiwygEditor(options: {
    matcher: (path: string) => boolean;
    component: EditorWysiwygComponent;
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
  /** Globally unique, e.g. "builtin.preview-markdown" or "plugin.<id>.editor". */
  id: string;
  /**
   * i18n key of the human title shown in the extension manager. Optional
   * when a direct `title` is supplied instead (plugin-delivered extensions
   * have no app i18n keys).
   */
  titleI18nKey?: string;
  /** Direct human title (plugin-delivered extensions; wins over the i18n key). */
  title?: string;
  /**
   * Opaque producer metadata. Only producers that can change without a code
   * change use it — the plugin bridge stores a `<id>@<version>` stamp so it
   * can tell whether an already-registered plugin extension needs replacing.
   */
  metadata?: Readonly<Record<string, string>>;
  contribute(ctx: EditorExtContext): void;
}
