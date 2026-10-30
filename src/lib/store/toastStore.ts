/**
 * Minimal self-built toast (no extra dependency). Failed actions toast and
 * surfaces keep user input — toasts are fire-and-forget notifications.
 *
 * Extended for ADR-0025 §2.3 Slice 3: supports `info` level, custom duration
 * (for 30s soft-undo), and optional action button.
 */
import { create } from "zustand";

export type ToastKind = "error" | "success" | "warn" | "info";

export interface ToastAction {
  label: string;
  onClick: () => void;
}

export interface ToastItem {
  id: number;
  kind: ToastKind;
  message: string;
  /** Action button (e.g. "撤回"). Toast auto-dismisses after action. */
  action?: ToastAction;
  /** Auto-dismiss duration in ms. Default 4000; soft-undo uses 30000. */
  durationMs?: number;
}

interface ToastState {
  toasts: ToastItem[];
  push: (kind: ToastKind, message: string, opts?: ToastPushOpts) => void;
  dismiss: (id: number) => void;
}

interface ToastPushOpts {
  action?: ToastAction;
  durationMs?: number;
}

let nextId = 1;

export const useToastStore = create<ToastState>((set) => ({
  toasts: [],
  push: (kind, message, opts) => {
    const id = nextId++;
    const durationMs = opts?.durationMs ?? 4000;
    set((s) => ({
      toasts: [
        ...s.toasts,
        { id, kind, message, action: opts?.action, durationMs },
      ],
    }));
    setTimeout(() => {
      set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
    }, durationMs);
  },
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

export const toast = {
  error: (message: string): void => useToastStore.getState().push("error", message),
  success: (message: string): void => useToastStore.getState().push("success", message),
  warn: (message: string): void => useToastStore.getState().push("warn", message),
  info: (message: string, opts?: ToastPushOpts): void =>
    useToastStore.getState().push("info", message, opts),
};
