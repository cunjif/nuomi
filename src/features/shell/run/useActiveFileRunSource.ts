/**
 * Hook: resolve the active editor file into a terminal run command.
 * Reads the active workspace + active file from uiStore, fetches the file
 * content and SDK configs, then maps the file language to a terminal command
 * via `languageToRunCommand`. Returns `command: null` when there is no active
 * file or the language has no matching command, so the entry UI can
 * disable the run button.
 */
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../../lib/ipc/client";
import { useUiStore } from "../../../lib/store/uiStore";
import { languageForPath } from "../editorLanguage";
import { languageToRunCommand } from "./languageToRuntime";
import { SDK_CONFIGS_KEY, parseSdkConfigs } from "./sdkConfigs";

export interface ActiveFileRunSource {
  /** Terminal command to run the active file, or null when no match. */
  command: string | null;
  /** Active file content (empty string when no active file). */
  source: string;
  /** Monaco language id for the active file (plaintext when unknown). */
  language: string;
  /** Active file path (null when no active file). */
  filePath: string | null;
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

  const sdkQuery = useQuery({
    queryKey: ["app-setting", SDK_CONFIGS_KEY],
    queryFn: () => ipc.appSettingGet(SDK_CONFIGS_KEY),
  });

  const source = activeFile !== null ? (fileQuery.data ?? "") : "";
  const language = activeFile !== null ? languageForPath(activeFile) : "plaintext";
  const configs = parseSdkConfigs(sdkQuery.data ?? null);
  const command = activeFile !== null ? languageToRunCommand(language, activeFile, configs) : null;

  return { command, source, language, filePath: activeFile };
}
