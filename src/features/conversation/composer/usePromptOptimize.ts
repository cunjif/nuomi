import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { ipc } from "../../../lib/ipc/client";
import { describeError } from "../../../i18n";
import { toast } from "../../../lib/store/toastStore";
import { useUiStore } from "../../../lib/store/uiStore";

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
 *
 * chat_rich_bind 5.6.1-3/6 (D9): both session kinds optimize through the
 * steward's bound model. `prompt.steward_unbound` routes the user to the
 * settings page instead of a generic failure toast; the draft stays.
 */
export function usePromptOptimize(sessionId: string | null) {
  const { t } = useTranslation();
  const setView = useUiStore((s) => s.setView);
  const [optimize, setOptimize] = useState<PromptOptimizeState>({ state: "idle" });

  const runOptimize = useCallback(
    async (draft: string): Promise<void> => {
      if (!sessionId || draft.trim().length === 0 || optimize.state === "running") return;
      setOptimize({ state: "running" });
      try {
        const result = await ipc.optimizePrompt(sessionId, draft);
        setOptimize({ state: "compare", original: draft, optimized: result.optimized });
      } catch (e) {
        const code = (e as { code?: unknown })?.code;
        if (code === "prompt.steward_unbound") {
          // Actionable nudge: bind the steward model in Settings (the
          // draft is untouched either way, spec 5.6.3-1c).
          toast.warn(t("composer.stewardUnbound"));
          setView("settings");
        }
        // Timeout/failure keeps the draft untouched (spec 5.3.3-3c).
        setOptimize({ state: "idle" });
        throw e;
      }
    },
    [sessionId, optimize.state, setView, t],
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
