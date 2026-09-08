/**
 * Builtin Markdown extension: two surfaces for .md files —
 * - WYSIWYG editor (Typora-style single pane, registered via
 *   registerWysiwygEditor; wins over Monaco when active);
 * - WYSIWYG-free split preview (Monaco left, rendered HTML right) for source
 *   mode. Rendering uses `marked` (tiny, no React dep) plus a small
 *   DOM-based sanitizer — workspace files can come from agents, so
 *   scripts/handlers/JS URLs are stripped before injecting HTML.
 */
import type { ReactNode } from "react";
import { useMemo } from "react";
import { marked } from "marked";
import type { EditorExtContext, EditorPreviewComponent } from "../types";
import { sanitizeMarkdownHtml } from "./sanitize";
import { WysiwygMarkdownEditor } from "./wysiwyg-markdown";

export const EXT_ID = "builtin.preview-markdown";

const MD_RE = /\.md$/i;

marked.setOptions({ gfm: true, breaks: true });

const MarkdownPreview: EditorPreviewComponent = ({ content }: { path: string; content: string }): ReactNode => {
  const html = useMemo(() => {
    const raw = marked.parse(content, { async: false });
    return sanitizeMarkdownHtml(raw);
  }, [content]);
  return (
    <div
      data-testid="markdown-preview"
      // Content is sanitized above (scripts/handlers/JS URLs removed).
      dangerouslySetInnerHTML={{ __html: html }}
      className="markdown-preview h-full min-w-0 flex-1 overflow-y-auto border-l border-ink-muted/30 bg-surface p-4 text-sm leading-6 text-ink [&_a]:text-ink-accent [&_a]:underline [&_blockquote]:border-l-4 [&_blockquote]:border-ink-muted/40 [&_blockquote]:pl-3 [&_blockquote]:text-ink-muted [&_code]:rounded [&_code]:bg-surface-overlay [&_code]:px-1 [&_code]:font-mono [&_h1]:mb-2 [&_h2]:mb-2 [&_h3]:mb-2 [&_h1]:mt-4 [&_h2]:mt-4 [&_h3]:mt-4 [&_h1]:font-semibold [&_h2]:font-semibold [&_h3]:font-semibold [&_pre]:overflow-x-auto [&_pre]:rounded [&_pre]:bg-surface-overlay [&_pre]:p-3 [&_table]:border-collapse [&_td]:border [&_th]:border [&_td]:border-ink-muted/40 [&_th]:border-ink-muted/40 [&_td]:px-2 [&_th]:px-2"
    />
  );
};

export const previewMarkdownExtension = {
  id: EXT_ID,
  titleI18nKey: "editor.ext.previewMarkdown",
  contribute(ctx: EditorExtContext): void {
    ctx.registerPreview({
      mode: "split",
      matcher: (path) => MD_RE.test(path),
      component: MarkdownPreview,
    });
    ctx.registerWysiwygEditor({
      matcher: (path) => MD_RE.test(path),
      component: WysiwygMarkdownEditor,
    });
    ctx.reportCapability("preview.markdown");
    ctx.reportCapability("wysiwyg.markdown");
  },
} as const;
