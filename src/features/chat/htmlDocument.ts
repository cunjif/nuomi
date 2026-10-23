/**
 * HTML document detection for chat bubbles (D3, conservative rule).
 *
 * A message counts as a full HTML document — eligible for the sandboxed
 * page preview — only when:
 *  1. the trimmed text starts with `<!doctype html` or `<html` (case-
 *     insensitive), or
 *  2. the FIRST fenced code block is a ```html fence whose content is
 *     itself a full document (the common "agent wraps the page in a code
 *     block" shape).
 *
 * Markdown messages that merely EMBED html fragments (`<div>` inside prose)
 * must NOT match — they render as regular markdown.
 */

const FULL_DOC_PREFIX = /^\s*(?:<!doctype\s+html|<html[\s>])/i;
const FENCED_HTML = /^```html\s*\n([\s\S]*?)\n?```\s*$/;

export interface DetectedHtmlDocument {
  /** The extractable HTML document source (unfenced when fence-wrapped). */
  source: string;
  /** True when the source came from a ```html fence (vs. raw text). */
  fenced: boolean;
}

/** Detects a full HTML document in a chat message (D3 rules). */
export function detectHtmlDocument(text: string): DetectedHtmlDocument | null {
  const trimmed = text.trim();
  if (trimmed.length === 0) return null;
  if (FULL_DOC_PREFIX.test(trimmed)) {
    return { source: trimmed, fenced: false };
  }
  // Only the FIRST fence may claim the preview, and it must span the whole
  // message (leading/trailing prose disqualifies — conservative).
  const fenceMatch = FENCED_HTML.exec(trimmed);
  if (fenceMatch !== null) {
    const inner = fenceMatch[1] ?? "";
    if (FULL_DOC_PREFIX.test(inner)) {
      return { source: inner, fenced: true };
    }
  }
  return null;
}

/** CSP meta injected ahead of the document content (D2): no scripts, no
 * network, inline styles only, data: images only. */
export const PREVIEW_CSP =
  "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:\">";

/** Injects the CSP meta as the first child of `<head>` (or prepends a `<head>`
 * when the document is malformed enough to lack one). */
export function injectPreviewCsp(source: string): string {
  const headOpen = /<head(\s[^>]*)?>/i.exec(source);
  if (headOpen !== null) {
    const at = headOpen.index + headOpen[0].length;
    return `${source.slice(0, at)}${PREVIEW_CSP}${source.slice(at)}`;
  }
  const htmlOpen = /<html(\s[^>]*)?>/i.exec(source);
  if (htmlOpen !== null) {
    const at = htmlOpen.index + htmlOpen[0].length;
    return `${source.slice(0, at)}<head>${PREVIEW_CSP}</head>${source.slice(at)}`;
  }
  return `${PREVIEW_CSP}${source}`;
}
