import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { ipc } from "../../../lib/ipc/client";
import { describeError } from "../../../i18n";

export type PromptOptimizeState =
  | { state: "idle" }
  | { state: "running" }
  | { state: "compare"; original: string; optimized: string };

/**
 * Shared prompt-optimize flow for every composer (steward task 4.7,
 * spec 5.3.1-4, generalized via the session-generic `optimizePrompt` IPC):
 * run → compare panel; adopt / retry / discard are handled by the caller
 * from the exposed state. Failures never touch the caller's draft — a
 * toast is raised and the state resets to idle.
 */
export function usePromptOptimize(sessionId: string | null) {
  const { t } = useTranslation();
  const [optimize, setOptimize] = useState<PromptOptimizeState>({ state: "idle" });

  const runOptimize = useCallback(
    async (draft: string): Promise<void> => {
      if (!sessionId || draft.trim().length === 0 || optimize.state === "running") return;
      setOptimize({ state: "running" });
      try {
        const result = await ipc.optimizePrompt(sessionId, draft);
        setOptimize({ state: "compare", original: draft, optimized: result.optimized });
      } catch (e) {
        // Timeout/failure keeps the draft untouched (spec 5.3.3-3c).
        setOptimize({ state: "idle" });
        throw e;
      }
    },
    [sessionId, optimize.state],
  );

  const clearOptimize = useCallback((): void => {
    setOptimize({ state: "idle" });
  }, []);

  const describeOptimizeError = useCallback(
    (e: unknown): string => t("composer.optimizeFailed", { message: describeError(e) }),
    [t],
  );

  return { optimize, runOptimize, clearOptimize, describeOptimizeError };
}
