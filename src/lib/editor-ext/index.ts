/**
 * Editor extension framework barrel: builtin extension list + re-exports.
 * Future Rust-plugin–delivered extensions hydrate through
 * registerEditorExtension() at startup (Cordis-mirror, see registry.ts).
 */
import type { EditorExtension } from "./types";
import { registerEditorExtension, ensureEditorExtensionsActivated } from "./registry";
import { outlineSymbolsExtension } from "./builtin/outline-symbols";
import { previewMarkdownExtension } from "./builtin/preview-markdown";
import { previewImageExtension } from "./builtin/preview-image";
import { previewPdfExtension } from "./builtin/preview-pdf";
import { previewOfficeExtension } from "./builtin/preview-office";
import { hoverDocsExtension } from "./builtin/hover-docs";
import { codebaseIndexExtension } from "./builtin/codebase-index";

/** Shipped-with-the-app extensions (individually disableable in the manager). */
export const builtinEditorExtensions: readonly EditorExtension[] = [
  outlineSymbolsExtension,
  previewMarkdownExtension,
  previewImageExtension,
  previewPdfExtension,
  previewOfficeExtension,
  hoverDocsExtension,
  codebaseIndexExtension,
];

/**
 * Register every builtin (idempotent — registerEditorExtension dedupes by
 * id) and run contribute() for the enabled ones. EditorArea calls this once
 * per mount before rendering children so preview matchers are queryable
 * during the first child render.
 */
export function activateBuiltinEditorExtensions(): void {
  for (const ext of builtinEditorExtensions) registerEditorExtension(ext);
  ensureEditorExtensionsActivated();
}

export {
  registerEditorExtension,
  getRegisteredEditorExtensions,
  ensureEditorExtensionsActivated,
  isEditorExtensionEnabled,
  setEditorExtensionEnabled,
  findPreviewForPath,
  getOutlineProviders,
  getToolbarActions,
  getOverlays,
  attachMonacoProviders,
  useEditorExtVersion,
  resetEditorExtensionsForTest,
} from "./registry";
export type {
  EditorExtension,
  EditorExtContext,
  EditorPreviewComponent,
  EditorPreviewMode,
  EditorPreviewProps,
  EditorToolbarAction,
  EditorOverlay,
  SymbolProvider,
  DocumentSymbol,
} from "./types";
