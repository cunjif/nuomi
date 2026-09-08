/**
 * Monaco themes matched to the nuomi design tokens so the editor canvas
 * blends with its container (surface-raised in both palettes). Type-only
 * import keeps the real monaco kernel out of tests and cold start.
 */
import type * as monaco from "monaco-editor";

export const NUOMI_MONACO_LIGHT = "nuomi-light";
export const NUOMI_MONACO_DARK = "nuomi-dark";

const lightTheme: monaco.editor.IStandaloneThemeData = {
  base: "vs",
  inherit: true,
  rules: [],
  colors: {
    "editor.background": "#ffffff",
    "editorGutter.background": "#ffffff",
    "editor.lineHighlightBackground": "#e8eaef",
  },
};

const darkTheme: monaco.editor.IStandaloneThemeData = {
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

/** Register both palettes once before the editor mounts. */
export function defineNuomiThemes(monaco: typeof import("monaco-editor")): void {
  monaco.editor.defineTheme(NUOMI_MONACO_LIGHT, lightTheme);
  monaco.editor.defineTheme(NUOMI_MONACO_DARK, darkTheme);
}
