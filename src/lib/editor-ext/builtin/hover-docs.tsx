/**
 * Builtin hover-docs extension: IDE-style Hover Info for code files —
 * hovering an identifier shows its declaration signature, where it is
 * declared (enclosing symbol + file:line), its attached doc comment and the
 * workspace reference count, mirroring the VSCode hover. Always on (the
 * old "注释" toolbar toggle was removed as a misread feature — hover info
 * is core editor behavior). LSP INTEGRATION POINT: a real language server
 * registers higher-priority providers through the same EditorExtContext —
 * this regex + index fallback stays as the offline baseline.
 */
import type * as Monaco from "monaco-editor";
import type { EditorExtContext } from "../types";
import { getOutlineProviders } from "../registry";
import { i18n } from "../../../i18n";
import { useUiStore } from "../../../lib/store/uiStore";
import { extractDocComment, getWorkspaceIndex } from "../indexer/workspace-index";

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

    const contents: Monaco.IMarkdownString[] = [];
    let signature: string | null = null;
    if (decl !== undefined) {
      signature = decl.signature;
      if (decl.doc !== null) contents.push({ value: decl.doc });
      const location = `${decl.path}:${decl.line + 1}`;
      contents.push({
        value:
          decl.container !== null
            ? i18n.t("editor.hover.declaredIn", { container: decl.container, location })
            : i18n.t("editor.hover.declaredInPath", { location }),
      });
    } else if (local !== undefined) {
      signature = (lines[local.range.start.line] ?? "").trim();
      const doc = extractDocComment(lines, local.range.start.line);
      if (doc !== null) contents.push({ value: doc });
    }
    if (signature !== null && signature.length > 0) {
      // Signature floats on top, VSCode-style.
      contents.unshift({ value: `\`\`\`${language}\n${signature}\n\`\`\`` });
    }
    const refCount = index.data?.references.get(word.word)?.length ?? 0;
    if (refCount > 0) {
      contents.push({ value: `---\n${i18n.t("editor.hover.refs", { count: refCount })}` });
    }
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
