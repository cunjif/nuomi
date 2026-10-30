/**
 * Run/build entry buttons embedded in the editor toolbar
 * (ADR-0025 scene A). Submits the active file or project directly to the
 * sandbox via `sandboxRunWorkbenchTarget` — the kernel reads the file
 * host-side and forces authorized dirs to the active workspace's mounted
 * directories. No manual code editor (scene C retired in Slice 1a).
 */
import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "../../../components/ui/Button";
import { ipc } from "../../../lib/ipc/client";
import { useUiStore } from "../../../lib/store/uiStore";
import { useActiveFileRunSource } from "./useActiveFileRunSource";

/** Sandbox probe timeout (spec 4.1.1): degrade to disabled after this. */
const PROBE_TIMEOUT_MS = 3000;

export function RunEntryButtons(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const setSettingsInitialTab = useUiStore((s) => s.setSettingsInitialTab);
  const setView = useUiStore((s) => s.setView);
  const activeFile = useUiStore((s) =>
    s.activeWorkspaceId !== null
      ? (s.editorByWorkspace[s.activeWorkspaceId]?.activeFile ?? null)
      : null,
  );

  const runtimesQuery = useQuery({
    queryKey: ["sandbox", "runtimes"],
    queryFn: () => ipc.sandboxListRuntimes(),
  });

  const [probeTimedOut, setProbeTimedOut] = useState(false);
  useEffect(() => {
    if (!runtimesQuery.isLoading) {
      setProbeTimedOut(false);
      return;
    }
    setProbeTimedOut(false);
    const timer = setTimeout(() => setProbeTimedOut(true), PROBE_TIMEOUT_MS);
    return () => clearTimeout(timer);
  }, [runtimesQuery.isLoading]);

  const { runtime } = useActiveFileRunSource();
  const runtimes = runtimesQuery.data ?? [];
  const unreachable = probeTimedOut || runtimesQuery.isError;
  const disabled = runtimes.length === 0 || unreachable || activeFile === null;

  const goConfigure = (): void => {
    setSettingsInitialTab("sandbox");
    setView("settings");
  };

  const runMut = useMutation({
    mutationFn: (vars: { targetPath: string; isProject: boolean }) =>
      ipc.sandboxRunWorkbenchTarget({
        targetPath: vars.targetPath,
        isProject: vars.isProject,
        runtime: runtime ?? runtimes[0]?.id ?? "python",
        limits: null,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["sandbox"] });
    },
  });

  return (
    <div className="flex items-center gap-1" data-testid="run-entry-buttons">
      <Button
        size="sm"
        disabled={disabled || runMut.isPending}
        onClick={() => activeFile && runMut.mutate({ targetPath: activeFile, isProject: false })}
        aria-label={t("workbench.run.runCode")}
      >
        {t("workbench.run.runCode")}
      </Button>
      <Button
        size="sm"
        variant="outline"
        disabled={disabled || runMut.isPending}
        onClick={() => activeFile && runMut.mutate({ targetPath: activeFile, isProject: true })}
        aria-label={t("workbench.run.buildApp")}
      >
        {t("workbench.run.buildApp")}
      </Button>
      {unreachable && (
        <button
          type="button"
          onClick={goConfigure}
          className="text-[10px] text-state-warn hover:underline"
          data-testid="sandbox-unreachable-hint"
        >
          {t("workbench.run.sandboxUnreachable")}
        </button>
      )}
    </div>
  );
}
