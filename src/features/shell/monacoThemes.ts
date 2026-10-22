/**
 * Monaco themes matched to the nuomi design tokens so the editor canvas
 * blends with its container. Five definitions mirror the hand-drawn theme
 * matrix (review §9 + eink-theme design); `monacoThemeFor` picks the right
 * one by app theme.
 * Type-only import keeps the real monaco kernel out of tests and cold start.
 *
 * Syntax rules (P2-E): every theme fills the same 12 token slots from a
 * per-theme palette, so highlighting stays in lockstep across themes. Slot
 * colors reuse the app's design-token hexes where possible — "ink & chalk
 * ink" on paper, pastel chalks on the blackboard, saturated primaries on
 * high-contrast; the few free picks are tuned for the theme's canvas.
 */
import type * as monaco from "monaco-editor";
import type { Theme } from "../../lib/store/uiStore";

export const NUOMI_MONACO_PAPER = "nuomi-paper-light";
export const NUOMI_MONACO_EINK = "nuomi-eink-paper";
export const NUOMI_MONACO_GRID = "nuomi-grid-notebook";
export const NUOMI_MONACO_CHALK = "nuomi-chalkboard-dark";
export const NUOMI_MONACO_HIGH = "nuomi-high-contrast";

/** Syntax slot → 6-digit hex (no '#'), one entry per slot per theme. */
interface SyntaxPalette {
  comment: string;
  keyword: string;
  string: string;
  number: string;
  constant: string;
  type: string;
  function: string;
  variable: string;
  tag: string;
  attributeName: string;
  attributeValue: string;
  metatag: string;
}

/** Shared token → slot mapping; all four themes fill the exact same slots. */
const SYNTAX_TOKENS: Array<{ token: string; slot: keyof SyntaxPalette; italic?: boolean }> = [
  { token: "comment", slot: "comment", italic: true },
  { token: "keyword", slot: "keyword" },
  { token: "string", slot: "string" },
  { token: "number", slot: "number" },
  { token: "constant", slot: "constant" },
  { token: "type", slot: "type" },
  { token: "function", slot: "function" },
  { token: "variable", slot: "variable" },
  { token: "tag", slot: "tag" },
  { token: "attribute.name", slot: "attributeName" },
  { token: "attribute.value", slot: "attributeValue" },
  { token: "metatag", slot: "metatag" },
];

const rulesFor = (p: SyntaxPalette): monaco.editor.ITokenThemeRule[] =>
  SYNTAX_TOKENS.map(({ token, slot, italic }) => ({
    token,
    foreground: p[slot],
    ...(italic ? { fontStyle: "italic" } : {}),
  }));

/** Theme data per app theme, keyed by the Theme union (single source). */
export const NUOMI_THEME_DATA: Record<Theme, monaco.editor.IStandaloneThemeData> = {
  // Kraft paper + colored inks: keyword uses the accent 棕红, strings the
  // ok green, numbers/literals the warn ochre, type/function the violet/blue
  // badge inks — five ink colors on kraft (paper-light tokens).
  "paper-light": {
    base: "vs",
    inherit: true,
    rules: rulesFor({
      comment: "8a8171",
      keyword: "9c3f0a",
      string: "166a2a",
      number: "8a6508",
      constant: "8a6508",
      type: "86198f",
      function: "0369a1",
      variable: "2a2520",
      tag: "9c3f0a",
      attributeName: "047857",
      attributeValue: "166a2a",
      metatag: "8a6508",
    }),
    // Kraft-paper raised tone from global.css paper-light (review §9.4).
    colors: { "editor.background": "#fffdf5", "editorGutter.background": "#fffdf5", "editor.lineHighlightBackground": "#eee8d9" },
  },
  // E-Ink paper: low-saturation ink colours on the neutral grey-white raised
  // surface — keyword/tag the deep cyan-ink, strings the ink green (= ok),
  // numbers/literals the ink ochre (= warn), type/function the muted violet/
  // blue greys, comment a plain neutral grey (eink-paper tokens, design §7).
  "eink-paper": {
    base: "vs",
    inherit: true,
    rules: rulesFor({
      comment: "8a8a8a",
      keyword: "14506e",
      string: "1e5f41",
      number: "80600a",
      constant: "80600a",
      type: "6d597a",
      function: "457b9d",
      variable: "1a1a1a",
      tag: "14506e",
      attributeName: "0f6138",
      attributeValue: "1e5f41",
      metatag: "80600a",
    }),
    // E-Ink raised paper tone from global.css eink-paper (design §3).
    colors: { "editor.background": "#f6f6f4", "editorGutter.background": "#f6f6f4", "editor.lineHighlightBackground": "#e2e2e0" },
  },
  // Engineer notebook: same ink family as paper-light, but the keyword/tag
  // slots take the notebook's accent blue (grid-notebook tokens).
  "grid-notebook": {
    base: "vs",
    inherit: true,
    rules: rulesFor({
      comment: "9099a3",
      keyword: "3150cb",
      string: "166a2a",
      number: "8a6508",
      constant: "8a6508",
      type: "86198f",
      function: "0369a1",
      variable: "1f2430",
      tag: "3150cb",
      attributeName: "047857",
      attributeValue: "166a2a",
      metatag: "8a6508",
    }),
    colors: { "editor.background": "#ffffff", "editorGutter.background": "#ffffff", "editor.lineHighlightBackground": "#eceef2" },
  },
  // Blackboard chalk: keyword/tag the accent blue chalk, strings the ok
  // green, numbers/literals the warn gold, type the pink-violet badge
  // chalk, functions the sky-blue badge chalk (chalkboard-dark tokens).
  "chalkboard-dark": {
    base: "vs-dark",
    inherit: true,
    rules: rulesFor({
      comment: "828a94",
      keyword: "5b9bf8",
      string: "9ece6a",
      number: "e0af68",
      constant: "e0af68",
      type: "f0abfc",
      function: "7dd3fc",
      variable: "e8e8e8",
      tag: "5b9bf8",
      attributeName: "6ee7b7",
      attributeValue: "9ece6a",
      metatag: "e0af68",
    }),
    colors: {
      // Hand-drawn blackboard palette: raised panel is #0d0d0d (global.css).
      "editor.background": "#0d0d0d",
      "editorGutter.background": "#0d0d0d",
      "editor.lineHighlightBackground": "#161616",
    },
  },
  // High-contrast: saturated primaries, one per slot family — keyword the
  // yellow accent, strings ok green, type/attribute the cap-re cyan, the
  // function slot pink-violet; numbers are the one free pick (bright
  // orange, luminance-matched to the theme's primaries).
  "high-contrast": {
    base: "vs-dark",
    inherit: true,
    rules: rulesFor({
      comment: "b0b0b0",
      keyword: "ffff00",
      string: "00ff00",
      number: "ff9e64",
      constant: "ff9e64",
      type: "66d9ff",
      function: "ff66ff",
      variable: "ffffff",
      tag: "ffff00",
      attributeName: "66d9ff",
      attributeValue: "00ff00",
      metatag: "ffff00",
    }),
    colors: {
      "editor.background": "#0a0a0a",
      "editorGutter.background": "#0a0a0a",
      "editor.lineHighlightBackground": "#1a1a1a",
    },
  },
};

/** Register all five palettes once before the editor mounts. */
export function defineNuomiThemes(m: typeof import("monaco-editor")): void {
  for (const theme of Object.keys(NUOMI_THEME_DATA) as Theme[]) {
    m.editor.defineTheme(monacoThemeFor(theme), NUOMI_THEME_DATA[theme]);
  }
}

/** Map the active app theme to its Monaco theme name. */
export function monacoThemeFor(theme: Theme): string {
  switch (theme) {
    case "paper-light":
      return NUOMI_MONACO_PAPER;
    case "eink-paper":
      return NUOMI_MONACO_EINK;
    case "grid-notebook":
      return NUOMI_MONACO_GRID;
    case "chalkboard-dark":
      return NUOMI_MONACO_CHALK;
    case "high-contrast":
      return NUOMI_MONACO_HIGH;
  }
}
