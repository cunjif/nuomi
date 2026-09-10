/**
 * Ephemeral UI state only (typescript-react rule: never duplicate IPC data
 * here — server data lives in TanStack Query).
 */
import { create } from "zustand";

export type View = "chat" | "board" | "trace" | "git" | "approvals" | "scheduler" | "settings" | "plugins";

/**
 * Which surface owns the main content area: the conversation view (whatever
 * `view` selects) or the full-area workspace editor. Mutually exclusive —
 * opening a workspace file flips to "editor", Alt+H toggles back.
 */
export type ActiveArea = "chat" | "editor";

/**
 * The four hand-drawn themes (review §9.2). `paper-light`/`grid-notebook` are
 * the light family (no `.dark` class); `chalkboard-dark`/`high-contrast` are
 * the dark family and toggle `.dark` on <html> so legacy `.dark`-scoped rules
 * still apply.
 */
export type Theme = "paper-light" | "grid-notebook" | "chalkboard-dark" | "high-contrast";

/** Stable display/cycle order for the 4 themes (review §9.2). */
export const THEME_ORDER: ReadonlyArray<Theme> = [
  "paper-light",
  "grid-notebook",
  "chalkboard-dark",
  "high-contrast",
];

/** Next theme when cycling (used by the legacy toggle affordance + command palette). */
export function nextTheme(theme: Theme): Theme {
  const idx = THEME_ORDER.indexOf(theme);
  return THEME_ORDER[(idx + 1) % THEME_ORDER.length] ?? theme;
}

/** localStorage key shared with the FOUC-prevention inline script in index.html. */
export const THEME_STORAGE_KEY = "nuomi.theme";

/**
 * Resolution order: stored preference → prefers-color-scheme → dark fallback.
 * Legacy `"light"`/`"dark"` values stored before the 4-theme migration are
 * mapped forward (review §9.6). Must stay in lockstep with the inline script
 * in index.html (FOUC defense).
 */
export function resolveInitialTheme(): Theme {
  try {
    const stored = localStorage.getItem(THEME_STORAGE_KEY);
    if (stored === "paper-light" || stored === "grid-notebook" || stored === "chalkboard-dark" || stored === "high-contrast") {
      return stored;
    }
    // Backward-compatible migration of the old two-value scheme.
    if (stored === "light") return "paper-light";
    if (stored === "dark") return "chalkboard-dark";
  } catch {
    // Storage unavailable (private mode / jsdom restrictions) — fall through.
  }
  if (typeof window.matchMedia === "function" && window.matchMedia("(prefers-color-scheme: light)").matches) {
    return "paper-light";
  }
  return "chalkboard-dark";
}

interface UiState {
  view: View;
  activeArea: ActiveArea;
  theme: Theme;
  selectedSessionId: string | null;
  openFiles: string[];
  activeFile: string | null;
  runDrawerTaskId: string | null;
  /** Volatile per-file edit dirtiness keyed by path (content itself lives in the Query cache). */
  dirtyPaths: Record<string, boolean>;
  setView: (view: View) => void;
  setActiveArea: (area: ActiveArea) => void;
  setTheme: (theme: Theme) => void;
  selectSession: (sessionId: string) => void;
  openFile: (path: string) => void;
  closeFile: (path: string) => void;
  setActiveFile: (path: string) => void;
  setRunDrawerTask: (taskId: string | null) => void;
  markDirty: (path: string, dirty: boolean) => void;
}

export const useUiStore = create<UiState>((set) => ({
  view: "chat",
  activeArea: "chat",
  theme: resolveInitialTheme(),
  selectedSessionId: null,
  openFiles: [],
  activeFile: null,
  runDrawerTaskId: null,
  dirtyPaths: {},
  setView: (view) => set({ view }),
  setActiveArea: (activeArea) => set({ activeArea }),
  // Pure state flip only — DOM class + persistence side effects live in useTheme.
  setTheme: (theme) => set({ theme }),
  selectSession: (sessionId) => set({ selectedSessionId: sessionId, activeArea: "chat" }),
  openFile: (path) =>
    set((s) => ({
      openFiles: s.openFiles.includes(path) ? s.openFiles : [...s.openFiles, path],
      activeFile: path,
      // Opening a workspace file always reveals the editor area (nav rework:
      // the editor is a full-area surface toggled with the 对话/文件编辑 tabs).
      activeArea: "editor",
    })),
  closeFile: (path) =>
    set((s) => {
      const openFiles = s.openFiles.filter((p) => p !== path);
      const dirtyPaths = { ...s.dirtyPaths };
      delete dirtyPaths[path];
      return {
        openFiles,
        dirtyPaths,
        activeFile:
          s.activeFile === path ? (openFiles.at(-1) ?? null) : s.activeFile,
      };
    }),
  setActiveFile: (path) => set({ activeFile: path }),
  setRunDrawerTask: (taskId) => set({ runDrawerTaskId: taskId }),
  // No-op when the flag already matches so per-keystroke onChange calls don't
  // churn subscribers.
  markDirty: (path, dirty) =>
    set((s) => {
      if ((s.dirtyPaths[path] ?? false) === dirty) return s;
      const dirtyPaths = { ...s.dirtyPaths };
      if (dirty) dirtyPaths[path] = true;
      else delete dirtyPaths[path];
      return { dirtyPaths };
    }),
}));
