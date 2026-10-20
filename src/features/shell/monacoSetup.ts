/**
 * Self-hosted Monaco wiring: bundled locally instead of the default CDN
 * loader so the desktop app works fully offline. Imported lazily right
 * before the editor chunk loads (keeps monaco out of cold-start & tests).
 *
 * Language-service web workers (TS/JSON/CSS/HTML) are split into their own
 * chunks via Vite's `?worker` imports and dispatched by language label —
 * without getWorker the services fail to load and fall back to the main
 * thread (no completions/diagnostics, console errors).
 */
import { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor";
import editorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import jsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";
import cssWorker from "monaco-editor/esm/vs/language/css/css.worker?worker";
import htmlWorker from "monaco-editor/esm/vs/language/html/html.worker?worker";
import tsWorker from "monaco-editor/esm/vs/language/typescript/ts.worker?worker";

const workerEnv: monaco.Environment = {
  getWorker(_workerId: string, label: string): Worker {
    if (label === "json") return new jsonWorker();
    if (label === "css" || label === "scss" || label === "less") return new cssWorker();
    if (label === "html" || label === "handlebars" || label === "razor") return new htmlWorker();
    if (label === "typescript" || label === "javascript") return new tsWorker();
    return new editorWorker();
  },
};

self.MonacoEnvironment = workerEnv;
loader.config({ monaco });
