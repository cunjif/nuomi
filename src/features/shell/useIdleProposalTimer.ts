import { useEffect } from "react";
import { ipc } from "../../lib/ipc/client";

/**
 * Idle team-proposal timer (ADR 0023). The frontend only measures liveness:
 * pointerdown/keydown at window CAPTURE (passive) reset a one-shot timeout;
 * when it fires we invoke `idle_check` once and let the Rust side decide
 * (config, backlog, pending-proposal and cooldown gates all live backend-side).
 * - Leaving the window unfocused keeps the timer running — time away counts
 *   as idle (decision 6), and any user activity re-arms it (decision 4).
 * - Advisory-only: `idle_check` failures are swallowed — a missed proposal
 *   is never worth an error toast.
 * - Runs solely inside the real Tauri webview (vitest renders go through
 *   the IPC test double and have no timer to arm); the effect cleanup
 *   disarms everything so HMR remounts never double-fire.
 */
export function useIdleProposalTimer(): void {
  useEffect(() => {
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
      return undefined;
    }
    const listenerOptions = { capture: true, passive: true } as const;
    const activityTypes = ["pointerdown", "keydown"] as const;
    let timer: number | undefined;
    let cancelled = false;
    const disposers: Array<() => void> = [];

    const arm = (timeoutMs: number): void => {
      if (timer !== undefined) window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        timer = undefined;
        void ipc.idleCheck().catch(() => { /* advisory-only — stay silent */ });
      }, timeoutMs);
    };

    void ipc.getIdleProposalSettings()
      .then((settings) => {
        if (cancelled || !settings.enabled || settings.timeoutMinutes <= 0) return;
        const timeoutMs = settings.timeoutMinutes * 60_000;
        const reset = (): void => arm(timeoutMs);
        for (const type of activityTypes) {
          window.addEventListener(type, reset, listenerOptions);
          disposers.push(() => window.removeEventListener(type, reset, listenerOptions));
        }
        arm(timeoutMs);
      })
      .catch(() => { /* settings unavailable — the timer simply stays off */ });

    return () => {
      cancelled = true;
      if (timer !== undefined) window.clearTimeout(timer);
      for (const dispose of disposers) dispose();
    };
  }, []);
}
