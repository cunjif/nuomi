import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";

// monaco-editor 0.56+ ships an `exports` map whose `./*` pattern prepends
// `./esm/vs/` to every subpath. Worker imports like
// `monaco-editor/esm/vs/editor/editor.worker?worker` get double-prefixed to
// `./esm/vs/esm/vs/editor/editor.worker.js` (non-existent) and Vite fails to
// resolve them. Aliasing the `esm/vs` subtree back to its filesystem path
// bypasses the `exports` map so the worker files resolve correctly.
const monacoEsmVs = fileURLToPath(
  new URL("./node_modules/monaco-editor/esm/vs", import.meta.url),
);

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  resolve: {
    alias: {
      "monaco-editor/esm/vs": monacoEsmVs,
    },
  },
  // Monaco language workers are ES modules (import/export); Vite's default
  // `iife` worker format cannot bundle them.
  worker: {
    format: "es",
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"] },
  },
  build: {
    target: "es2022",
    outDir: "dist",
  },
});
