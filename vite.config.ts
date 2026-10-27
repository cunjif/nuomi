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
  // monaco-editor must NOT be pre-bundled: its `?worker` imports (language
  // services) are handled by Vite's worker plugin, not esbuild. When
  // monaco-editor is pre-bundled, import-analysis rewrites `?worker` imports
  // to `/node_modules/.vite/deps/..._worker?worker.js` URLs, but esbuild never
  // generates those artifacts (it ignores the `?worker` query) → 404 →
  // "Failed to fetch dynamically imported module: .../monacoSetup.ts" when the
  // editor lazy-loads. Excluding it lets the worker plugin process `?worker`
  // imports directly against the aliased source files. @monaco-editor/react
  // only has monaco-editor as a peer dep (via loader), so its pre-bundle is
  // unaffected.
  optimizeDeps: {
    exclude: ["monaco-editor"],
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
