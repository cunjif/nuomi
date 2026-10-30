/**
 * Builtin hover-docs extension: IDE-style Hover Info for code files —
 * hovering an identifier shows its declaration signature, where it is
 * declared (enclosing symbol + file:line), its attached doc comment and the
 * workspace reference count, mirroring the VSCode hover. Always on (the
 * old "注释" toolbar toggle was removed as a misread feature — hover info
 * is core editor behavior). LSP INTEGRATION POINT: a real language server
 * registers higher-priority providers through the same EditorExtContext —
 * this regex + index fallback stays as the offline baseline.
 *
 * workbench_enhance spec 5.3: enhanced with per-kind差异化呈现
 * (parseParameters / parseReturnValue / composeHoverContent).
 */
import type * as Monaco from "monaco-editor";
import type { EditorExtContext } from "../types";
import { getOutlineProviders, isEditorExtensionEnabled } from "../registry";
import { i18n } from "../../../i18n";
import { useUiStore } from "../../../lib/store/uiStore";
import { extractDocComment, getWorkspaceIndex } from "../indexer/workspace-index";
import { composeHoverContent, parseParameters, parseReturnValue } from "./hover-parse";

export const EXT_ID = "builtin.hover-docs";

function activeFilePath(): string {
  const s = useUiStore.getState();
  const wsId = s.activeWorkspaceId;
  return wsId !== null ? (s.editorByWorkspace[wsId]?.activeFile ?? "") : "";
}

const hoverProvider: Monaco.languages.HoverProvider = {
  provideHover(model: Monaco.editor.ITextModel, position: Monaco.Position): Monaco.languages.Hover | null {
    const word = model.getWordAtPosition(position);
    if (word === null) return null;
    const language = model.getLanguageId();
    const content = model.getValue();
    const lines = content.split(/\r?\n/);
    // In-file symbols come from the outline registry; the workspace index
    // adds cross-file declarations + reference counts.
    const local = getOutlineProviders()
      .filter((p) => p.languages.includes("*") || p.languages.includes(language))
      .flatMap((p) => p.provideSymbols({ path: activeFilePath(), language, content }))
      .find((s) => s.name === word.word);
    const index = getWorkspaceIndex();
    const decl = index.data?.definitions.get(word.word)?.[0];

    let signature: string | null = null;
    let doc: string | null = null;
    let kind = "function";
    let container: string | null = null;
    let location: string | null = null;

    if (decl !== undefined) {
      signature = decl.signature;
      doc = decl.doc;
      kind = decl.kind;
      container = decl.container;
      location = `${decl.path}:${decl.line + 1}`;
    } else if (local !== undefined) {
      signature = (lines[local.range.start.line] ?? "").trim();
      doc = extractDocComment(lines, local.range.start.line);
      kind = local.kind;
    }

    if (signature === null || signature.length === 0) {
      // No signature → only show refs if any.
      const refCount = index.data?.references.get(word.word)?.length ?? 0;
      if (refCount === 0) return null;
      return { contents: [{ value: `---\n${i18n.t("editor.hover.refs", { count: refCount })}` }] };
    }

    const refCount = index.data?.references.get(word.word)?.length ?? 0;

    // Fallback: when enhanced hover is disabled, render basic content
    // (signature + doc + location + refs) without parameter/return parsing.
    if (!isEditorExtensionEnabled("workbench.enhancedHover")) {
      const parts: Monaco.IMarkdownString[] = [];
      parts.push({ value: "```" + language + "\n" + signature + "\n```" });
      if (doc !== null && doc.length > 0) parts.push({ value: doc });
      if (location !== null) parts.push({ value: `---\n${i18n.t("editor.hover.declaredInPath", { location })}` });
      if (refCount > 0) parts.push({ value: `---\n${i18n.t("editor.hover.refs", { count: refCount })}` });
      return parts.length > 0 ? { contents: parts } : null;
    }

    const params = parseParameters(signature, doc, language);
    const returnValue = parseReturnValue(signature, doc, language);
    const contents = composeHoverContent({
      kind,
      signature,
      doc,
      params,
      returnValue,
      location,
      container,
      refCount,
      language,
    });
    return contents.length > 0 ? { contents } : null;
  },
};

export const hoverDocsExtension = {
  id: EXT_ID,
  titleI18nKey: "editor.ext.hoverDocs",
  contribute(ctx: EditorExtContext): void {
    for (const language of ["typescript", "javascript", "rust", "python", "java", "go", "c", "cpp"]) {
      ctx.registerHoverProvider(language, hoverProvider);
    }
    ctx.reportCapability("hover.docs");
  },
} as const;
