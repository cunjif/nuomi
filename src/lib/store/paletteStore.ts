/**
 * Palette overlay state (Ctrl+P / Ctrl+Shift+P, VSCode-style). Pure UI
 * toggle state — file/command data lives in the QuickOpen component's own
 * queries and registries.
 */
import { create } from "zustand";

export type PaletteMode = "files" | "commands";

interface PaletteState {
  /** null = closed. */
  mode: PaletteMode | null;
  open: (mode: PaletteMode) => void;
  close: () => void;
}

export const usePaletteStore = create<PaletteState>((set) => ({
  mode: null,
  open: (mode) => set({ mode }),
  close: () => set({ mode: null }),
}));
