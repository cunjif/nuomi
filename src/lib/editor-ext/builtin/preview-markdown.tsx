/**
 * Builtin Markdown preview: WYSIWYG split view (Monaco left, rendered HTML
 * right) for .md files. Rendering uses `marked` (tiny, no React dep) plus a
 * small DOM-based sanitizer — workspace files can come from agents, so
 * scripts/handlers/JS URLs are stripped before injecting HTML.
 */
import type { ReactNode } from "react";
import { useMemo } from "react";
import { marked } from "marked";
import type { EditorExtContext, EditorPreviewComponent } from "../types";

export const EXT_ID = "builtin.preview-markdown";

const MD_RE = /\.md$/i;

marked.setOptions({ gfm: true, breaks: true });

/**
 * Minimal allowlist-style sanitizer (no external dep): drops executable
 * elements/attributes from marked's HTML output. Good enough for a desktop
 * preview; a stricter DOMPurify-based pipeline can arrive as a third-party
 * editor extension later.
 */
export function sanitizeMarkdownHtml(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  for (const el of [...doc.body.querySelectorAll("script, style, iframe, object, embed, link, meta")]) {
    el.remove();
  }
  for (const el of [...doc.body.querySelectorAll("*")]) {
    for (const attr of [...el.attributes]) {
      const name = attr.name.toLowerCase();
      if (name.startsWith("on")) {
        el.removeAttribute(attr.name);
      } else if ((name === "href" || name === "src") && /^\s*javascript:/i.test(attr.value)) {
        el.removeAttribute(attr.name);
      }
    }
  }
  return doc.body.innerHTML;
}

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
    ctx.reportCapability("preview.markdown");
  },
} as const;
