/**
 * Hook: resolve the active editor file into a sandbox run source
 * (workbench_enhance spec 5.1.1 / design 2.2.2). Reads the active
 * workspace + active file from uiStore, fetches the file content and the
 * sandbox runtime list, then maps the file language to a runtime id via
 * `languageToRuntime`. Returns `runtime: null` when there is no active
 * file or the language has no matching runtime, so the entry UI can
 * require an explicit runtime + source selection.
 */
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../../lib/ipc/client";
import { useUiStore } from "../../../lib/store/uiStore";
import { languageForPath } from "../editorLanguage";
import { languageToRuntime } from "./languageToRuntime";

export interface ActiveFileRunSource {
  /** Matched runtime id, or null when no active file / no matching runtime. */
  runtime: string | null;
  /** Active file content (empty string when no active file). */
  source: string;
  /** Monaco language id for the active file (plaintext when unknown). */
  language: string;
}

export function useActiveFileRunSource(): ActiveFileRunSource {
  const activeWorkspaceId = useUiStore((s) => s.activeWorkspaceId);
  const activeFile = useUiStore((s) =>
    s.activeWorkspaceId !== null
      ? (s.editorByWorkspace[s.activeWorkspaceId]?.activeFile ?? null)
      : null,
  );

  const fileQuery = useQuery({
    queryKey: ["file", activeWorkspaceId, activeFile],
    queryFn: () => ipc.readFile(activeFile ?? "", activeWorkspaceId ?? null),
    enabled: activeWorkspaceId !== null && activeFile !== null,
  });

  const runtimesQuery = useQuery({
    queryKey: ["sandbox", "runtimes"],
    queryFn: () => ipc.sandboxListRuntimes(),
  });

  const source = activeFile !== null ? (fileQuery.data ?? "") : "";
  const language = activeFile !== null ? languageForPath(activeFile) : "plaintext";
  const runtimes = runtimesQuery.data ?? [];
  const runtime = languageToRuntime(language, runtimes);

  return { runtime, source, language };
}
