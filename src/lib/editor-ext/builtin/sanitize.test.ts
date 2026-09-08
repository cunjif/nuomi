/**
 * Sanitizer tests: the allowlist is what stands between workspace markdown
 * and dangerouslySetInnerHTML, so every rejection path is pinned here.
 */
import { describe, expect, it } from "vitest";
import { sanitizeMarkdownHtml } from "./sanitize";

describe("sanitizeMarkdownHtml", () => {
  it("keeps ordinary markdown output intact", () => {
    const html = '<h2 id="t">Hi</h2>\n<p>a <strong>b</strong> <a href="https://x.dev">l</a></p>\n<pre><code class="language-ts">x</code></pre>';
    expect(sanitizeMarkdownHtml(html)).toBe(html);
  });

  it("removes script/style/iframe together with their content", () => {
    expect(sanitizeMarkdownHtml('<p>a</p><script>alert(1)</script>')).toBe("<p>a</p>");
    expect(sanitizeMarkdownHtml("<style>body{display:none}</style><p>a</p>")).toBe("<p>a</p>");
    expect(sanitizeMarkdownHtml('<iframe src="https://evil"></iframe><p>a</p>')).toBe("<p>a</p>");
  });

  it("strips event handlers", () => {
    expect(sanitizeMarkdownHtml('<p onclick="alert(1)">a</p>')).toBe("<p>a</p>");
    expect(sanitizeMarkdownHtml('<img src="x" onerror="alert(1)" alt="a" />')).toBe('<img src="x" alt="a">');
  });

  it("strips unsafe URL schemes but keeps http(s)/mailto/relative/anchor", () => {
    expect(sanitizeMarkdownHtml('<a href="javascript:alert(1)">x</a>')).toBe("<a>x</a>");
    expect(sanitizeMarkdownHtml('<a href="vbscript:msgbox">x</a>')).toBe("<a>x</a>");
    expect(sanitizeMarkdownHtml('<a href="data:text/html,<script>1</script>">x</a>')).toBe("<a>x</a>");
    expect(sanitizeMarkdownHtml('<a href="//evil.dev">x</a>')).toBe("<a>x</a>");
    expect(sanitizeMarkdownHtml('<a href="./a.md">x</a>')).toBe('<a href="./a.md">x</a>');
    expect(sanitizeMarkdownHtml('<a href="#sec">x</a>')).toBe('<a href="#sec">x</a>');
    expect(sanitizeMarkdownHtml('<a href="mailto:a@b.c">x</a>')).toBe('<a href="mailto:a@b.c">x</a>');
  });

  it("allows inline base64 images only for image src", () => {
    const img = '<img src="data:image/png;base64,AAAA" alt="a">';
    expect(sanitizeMarkdownHtml(img)).toBe(img);
    expect(sanitizeMarkdownHtml('<img src="data:text/html,x" alt="a">')).toBe('<img alt="a">');
  });

  it("drops style attributes and unwraps unknown/form elements", () => {
    expect(sanitizeMarkdownHtml('<p style="background:url(javascript:1)">a</p>')).toBe("<p>a</p>");
    expect(sanitizeMarkdownHtml("<form action=\"/x\"><p>a</p></form>")).toBe("<p>a</p>");
    expect(sanitizeMarkdownHtml("<my-widget><p>a</p></my-widget>")).toBe("<p>a</p>");
  });
});
