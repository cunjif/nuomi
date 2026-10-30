/**
 * Pure mapping from a Monaco language id to a sandbox runtime id
 * (workbench_enhance spec 5.1.1 / design 2.2.2). The returned id must
 * exist in the runtime list; unmatched languages yield null so the caller
 * can prompt the user to pick a runtime explicitly.
 */
import type { SandboxRuntimeDto } from "../../../lib/ipc/bindings.gen";

/** Monaco language id → candidate sandbox runtime id. */
const LANGUAGE_TO_RUNTIME_ID: Record<string, string> = {
  python: "python",
  javascript: "node",
  typescript: "node",
  rust: "rust",
  go: "go",
  java: "java",
  c: "c",
  cpp: "cpp",
};

/**
 * Resolve a Monaco language id to a sandbox runtime id present in `runtimes`.
 * Returns null when the language has no mapping or the mapped runtime is not
 * registered, so the entry UI can require an explicit runtime selection.
 */
export function languageToRuntime(
  language: string,
  runtimes: SandboxRuntimeDto[],
): string | null {
  const candidate = LANGUAGE_TO_RUNTIME_ID[language];
  if (candidate === undefined) return null;
  return runtimes.some((r) => r.id === candidate) ? candidate : null;
}
