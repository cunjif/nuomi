/**
 * Minimal self-built toast (no extra dependency). Failed actions toast and
 * surfaces keep user input — toasts are fire-and-forget notifications.
 */
import { create } from "zustand";

export type ToastKind = "error" | "success" | "warn";

export interface ToastItem {
  id: number;
  kind: ToastKind;
  message: string;
}

interface ToastState {
  toasts: ToastItem[];
  push: (kind: ToastKind, message: string) => void;
  dismiss: (id: number) => void;
}

let nextId = 1;

export const useToastStore = create<ToastState>((set) => ({
  toasts: [],
  push: (kind, message) => {
    const id = nextId++;
    set((s) => ({ toasts: [...s.toasts, { id, kind, message }] }));
    setTimeout(() => {
      set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
    }, 4000);
  },
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));

export const toast = {
  error: (message: string): void => useToastStore.getState().push("error", message),
  success: (message: string): void => useToastStore.getState().push("success", message),
  warn: (message: string): void => useToastStore.getState().push("warn", message),
};
