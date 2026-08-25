/**
 * Ephemeral UI state only (typescript-react rule: never duplicate IPC data
 * here — server data lives in TanStack Query).
 */
import { create } from "zustand";

export type View = "chat" | "board" | "trace" | "git" | "approvals" | "scheduler" | "settings";

interface UiState {
  view: View;
  selectedSessionId: string | null;
  openFiles: string[];
  activeFile: string | null;
  runDrawerTaskId: string | null;
  setView: (view: View) => void;
  selectSession: (sessionId: string) => void;
  openFile: (path: string) => void;
  closeFile: (path: string) => void;
  setActiveFile: (path: string) => void;
  setRunDrawerTask: (taskId: string | null) => void;
}

export const useUiStore = create<UiState>((set) => ({
  view: "chat",
  selectedSessionId: null,
  openFiles: [],
  activeFile: null,
  runDrawerTaskId: null,
  setView: (view) => set({ view }),
  selectSession: (sessionId) => set({ selectedSessionId: sessionId }),
  openFile: (path) =>
    set((s) => ({
      openFiles: s.openFiles.includes(path) ? s.openFiles : [...s.openFiles, path],
      activeFile: path,
    })),
  closeFile: (path) =>
    set((s) => {
      const openFiles = s.openFiles.filter((p) => p !== path);
      return {
        openFiles,
        activeFile:
          s.activeFile === path ? (openFiles.at(-1) ?? null) : s.activeFile,
      };
    }),
  setActiveFile: (path) => set({ activeFile: path }),
  setRunDrawerTask: (taskId) => set({ runDrawerTaskId: taskId }),
}));
