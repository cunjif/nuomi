/**
 * Monaco themes matched to the nuomi design tokens so the editor canvas
 * blends with its container. Four definitions mirror the hand-drawn theme
 * matrix (review §9); `monacoThemeFor` picks the right one by app theme.
 * Type-only import keeps the real monaco kernel out of tests and cold start.
 */
import type * as monaco from "monaco-editor";
import type { Theme } from "../../lib/store/uiStore";

export const NUOMI_MONACO_PAPER = "nuomi-paper-light";
export const NUOMI_MONACO_GRID = "nuomi-grid-notebook";
export const NUOMI_MONACO_CHALK = "nuomi-chalkboard-dark";
export const NUOMI_MONACO_HIGH = "nuomi-high-contrast";

const paperLight: monaco.editor.IStandaloneThemeData = {
  base: "vs",
  inherit: true,
  rules: [],
  // Kraft-paper raised tone from global.css paper-light (review §9.4).
  colors: { "editor.background": "#fffdf5", "editorGutter.background": "#fffdf5", "editor.lineHighlightBackground": "#eee8d9" },
};

const gridNotebook: monaco.editor.IStandaloneThemeData = {
  base: "vs",
  inherit: true,
  rules: [],
  colors: { "editor.background": "#ffffff", "editorGutter.background": "#ffffff", "editor.lineHighlightBackground": "#eceef2" },
};

const chalkboardDark: monaco.editor.IStandaloneThemeData = {
  base: "vs-dark",
  inherit: true,
  rules: [],
  colors: {
    // Hand-drawn blackboard palette: raised panel is #0d0d0d (global.css).
    "editor.background": "#0d0d0d",
    "editorGutter.background": "#0d0d0d",
    "editor.lineHighlightBackground": "#161616",
  },
};

const highContrast: monaco.editor.IStandaloneThemeData = {
  base: "vs-dark",
  inherit: true,
  rules: [],
  colors: {
    "editor.background": "#0a0a0a",
    "editorGutter.background": "#0a0a0a",
    "editor.lineHighlightBackground": "#1a1a1a",
  },
};

/** Register all four palettes once before the editor mounts. */
export function defineNuomiThemes(m: typeof import("monaco-editor")): void {
  m.editor.defineTheme(NUOMI_MONACO_PAPER, paperLight);
  m.editor.defineTheme(NUOMI_MONACO_GRID, gridNotebook);
  m.editor.defineTheme(NUOMI_MONACO_CHALK, chalkboardDark);
  m.editor.defineTheme(NUOMI_MONACO_HIGH, highContrast);
}

/** Map the active app theme to its Monaco theme name. */
export function monacoThemeFor(theme: Theme): string {
  switch (theme) {
    case "paper-light":
      return NUOMI_MONACO_PAPER;
    case "grid-notebook":
      return NUOMI_MONACO_GRID;
    case "chalkboard-dark":
      return NUOMI_MONACO_CHALK;
    case "high-contrast":
      return NUOMI_MONACO_HIGH;
  }
}
