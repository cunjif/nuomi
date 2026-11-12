/**
 * The shared monospace stack (single source: `--font-mono` in global.css,
 * also used by the `font-mono` class and `.md-wysiwyg code`).
 *
 * Canvas-based text measurement — Monaco and xterm.js both measure glyph
 * widths with a 2D context — cannot resolve a `var()` reference: assigning
 * `ctx.font = '13px var(--font-mono)'` is an invalid font shorthand and
 * silently falls back to a default face. Components must therefore read the
 * computed stack and pass a concrete family list.
 */
export function monoFontStack(): string {
  const stack = getComputedStyle(document.documentElement)
    .getPropertyValue("--font-mono")
    .trim();
  return stack !== "" ? stack : "monospace";
}