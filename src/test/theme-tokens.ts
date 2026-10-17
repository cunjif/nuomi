/** Raw Tailwind palette classes (red-500, bg-black/10, text-white, …) never
 *  resolve through the theme CSS vars, so they break the dual-theme contract
 *  and the themeContrast gate's assumptions (src/styles/themeContrast.test.ts
 *  guards the palette values; this scans the rendered component tree). Every
 *  component must ride the semantic tokens (surface/ink/state/divider). */
const RAW_PALETTE_CLASS =
  /\b(?:text|bg|border|ring|fill|stroke|shadow|from|to|via|outline|decoration|divide|caret|placeholder)-(?:red|blue|yellow|green|black|white|gray|grey|slate|zinc|neutral|stone|amber|lime|emerald|teal|cyan|sky|indigo|violet|purple|fuchsia|pink|rose|orange)(?:-\d+)?\b/;

/** Returns every raw palette class found under `root` (empty when the tree is
 *  fully token-driven). */
export function scanRawPaletteClasses(root: HTMLElement): string[] {
  const offenders: string[] = [];
  for (const el of root.querySelectorAll("*")) {
    const cls = el.getAttribute("class");
    if (cls !== null && RAW_PALETTE_CLASS.test(cls)) offenders.push(cls);
  }
  return offenders;
}
