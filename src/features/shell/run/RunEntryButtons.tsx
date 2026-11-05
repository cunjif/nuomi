/**
 * Run/build entry buttons embedded in the editor toolbar.
 * Clicking "run code" or "build app" switches to the terminal area,
 * creates a new terminal tab bound to the workspace directory, and
 * executes the constructed command in that terminal.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { Button } from "../../../components/ui/Button";
import { ipc } from "../../../lib/ipc/client";
import { useUiStore, type TerminalInstance } from "../../../lib/store/uiStore";
import { useActiveFileRunSource } from "./useActiveFileRunSource";
import { detectBuildCommand } from "./languageToRuntime";
import { SDK_CONFIGS_KEY, parseSdkConfigs } from "./sdkConfigs";

export function RunEntryButtons(): ReactNode {
  const { t } = useTranslation();
  const setActiveArea = useUiStore((s) => s.setActiveArea);
  const addTerminal = useUiStore((s) => s.addTerminal);
  const activeWorkspaceId = useUiStore((s) => s.activeWorkspaceId);
  const activeFile = useUiStore((s) =>
    s.activeWorkspaceId !== null
      ? (s.editorByWorkspace[s.activeWorkspaceId]?.activeFile ?? null)
      : null,
  );

  const { command } = useActiveFileRunSource();

  const rootFilesQuery = useQuery({
    queryKey: ["dir", activeWorkspaceId, ""],
    queryFn: () => ipc.listDir("", activeWorkspaceId ?? null),
    enabled: activeWorkspaceId !== null,
  });

  const sdkQuery = useQuery({
    queryKey: ["app-setting", SDK_CONFIGS_KEY],
    queryFn: () => ipc.appSettingGet(SDK_CONFIGS_KEY),
  });

  const rootFiles = (rootFilesQuery.data ?? []).map((e) => e.name);
  const configs = parseSdkConfigs(sdkQuery.data ?? null);
  const buildCommand = detectBuildCommand(rootFiles, configs);

  const runDisabled = activeFile === null || command === null;
  const buildDisabled = buildCommand === null;

  function createRunTerminal(cmd: string, title: string): void {
    if (activeWorkspaceId === null) return;
    const id = `term-${Date.now()}`;
    const instance: TerminalInstance = {
      id,
      title,
      createdAt: Date.now(),
      cwdSource: "workspace",
      initialCommand: cmd,
    };
    addTerminal(activeWorkspaceId, instance);
    setActiveArea("terminal");
  }

  return (
    <div className="flex items-center gap-1" data-testid="run-entry-buttons">
      <Button
        size="sm"
        disabled={runDisabled}
        onClick={() => command !== null && createRunTerminal(command, t("workbench.run.runCode"))}
        aria-label={t("workbench.run.runCode")}
      >
        {t("workbench.run.runCode")}
      </Button>
      <Button
        size="sm"
        variant="outline"
        disabled={buildDisabled}
        onClick={() => buildCommand !== null && createRunTerminal(buildCommand, t("workbench.run.buildApp"))}
        aria-label={t("workbench.run.buildApp")}
      >
        {t("workbench.run.buildApp")}
      </Button>
    </div>
  );
}
