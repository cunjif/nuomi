/**
 * Allowlist HTML sanitizer shared by the markdown preview and the WYSIWYG
 * editor (no external dep): only known-safe tags/attributes/URL schemes
 * survive, so workspace markdown — which can come from agents or be edited
 * by hand — is inert before it is injected with dangerouslySetInnerHTML.
 *
 * Design notes:
 * - Allowlist, not blocklist: unknown tags are unwrapped (children kept, so
 *   legitimate markdown nesting still renders) and unknown attributes are
 *   dropped. `style` is therefore gone too (CSS injection via
 *   `background:url(javascript:)` and friends).
 * - Elements whose *content* is never safe (script/style/… including their
 *   text) are removed outright instead of unwrapped.
 */

const DROP_TAGS = new Set([
  "script",
  "style",
  "noscript",
  "iframe",
  "object",
  "embed",
  "template",
  "link",
  "meta",
  "base",
  "svg",
  "math",
]);

const ALLOWED_TAGS = new Set([
  "p", "br", "hr", "span", "div",
  "h1", "h2", "h3", "h4", "h5", "h6",
  "strong", "b", "em", "i", "del", "s", "ins", "u", "sub", "sup", "mark", "kbd", "small",
  "ul", "ol", "li", "blockquote", "pre", "code",
  "a", "img",
  "table", "thead", "tbody", "tfoot", "tr", "td", "th", "colgroup", "col",
  "input", "details", "summary", "dl", "dt", "dd", "figure", "figcaption",
]);

const ALLOWED_ATTRS = new Set([
  "href", "src", "alt", "title", "class", "id",
  "colspan", "rowspan", "align", "width", "height",
  "start", "type", "checked", "disabled", "open", "value", "role",
]);

/** Attributes carrying a URL — validated against the scheme allowlist. */
const URL_ATTRS = new Set(["href", "src"]);

/**
 * True for URLs we are willing to leave in the DOM: http(s), mailto, in-page
 * anchors, relative paths and (for `src` only) inline images. Everything else
 * — `javascript:`, `vbscript:`, `data:text/html`, `file:`, protocol-relative
 * `//evil` — is rejected.
 */
function isSafeUrl(raw: string, allowInlineImage: boolean): boolean {
  const url = raw.trim();
  if (url === "" || url.startsWith("#") || url.startsWith("?")) return true;
  if (/^https?:\/\//i.test(url) || /^mailto:/i.test(url)) return true;
  if (allowInlineImage && /^data:image\/(png|jpe?g|gif|webp|avif);base64,/i.test(url)) return true;
  // Any other "scheme:" prefix is rejected, as is protocol-relative.
  return !/^[a-z][a-z0-9+.-]*:/i.test(url) && !url.startsWith("//");
}

/** Replace an element with its children (keeps text, drops the wrapper). */
function unwrap(el: Element): void {
  const parent = el.parentNode;
  if (parent === null) {
    el.remove();
    return;
  }
  while (el.firstChild !== null) parent.insertBefore(el.firstChild, el);
  parent.removeChild(el);
}

export function sanitizeMarkdownHtml(html: string): string {
  const doc = new DOMParser().parseFromString(html, "text/html");
  for (const el of [...doc.body.querySelectorAll("*")]) {
    // A dropped/unwrapped ancestor already detached this node.
    if (!el.isConnected) continue;
    const tag = el.tagName.toLowerCase();
    if (DROP_TAGS.has(tag)) {
      el.remove();
      continue;
    }
    if (!ALLOWED_TAGS.has(tag)) {
      unwrap(el);
      continue;
    }
    for (const attr of [...el.attributes]) {
      const name = attr.name.toLowerCase();
      if (URL_ATTRS.has(name)) {
        if (!isSafeUrl(attr.value, name === "src")) el.removeAttribute(attr.name);
        continue;
      }
      if (!ALLOWED_ATTRS.has(name) && !name.startsWith("aria-") && !name.startsWith("data-")) {
        el.removeAttribute(attr.name);
      }
    }
  }
  return doc.body.innerHTML;
}
