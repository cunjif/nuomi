/**
 * Ephemeral UI state only (typescript-react rule: never duplicate IPC data
 * here — server data lives in TanStack Query).
 */
import { create } from "zustand";

export type View = "chat" | "board" | "trace" | "git" | "approvals" | "scheduler" | "settings" | "plugins";

/**
 * Which surface owns the main content area: the conversation view (whatever
 * `view` selects) or the workbench (workspace list + file editor). Mutually
 * exclusive — opening a workspace file flips to "workbench" with sub-tab
 * "editor", Alt+H toggles back to "chat".
 */
export type ActiveArea = "chat" | "workbench";

/**
 * Second-level tab within the workbench area: the workspace list panel or
 * the single-workspace file editor. Persisted alongside `activeArea` so
 * Alt+H round-trips restore the last workbench sub-tab.
 */
export type WorkbenchSubTab = "workspaceList" | "editor";

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

/** Per-workspace editor tab snapshot, saved on switch and restored on return. */
interface WorkspaceTabSnapshot {
  openFiles: string[];
  activeFile: string | null;
}

interface UiState {
  view: View;
  activeArea: ActiveArea;
  workbenchSubTab: WorkbenchSubTab;
  theme: Theme;
  selectedSessionId: string | null;
  openFiles: string[];
  activeFile: string | null;
  runDrawerTaskId: string | null;
  /** Volatile per-file edit dirtiness keyed by path (content itself lives in the Query cache). */
  dirtyPaths: Record<string, boolean>;
  /** Currently active workspace id (mirrors the backend active workspace). */
  activeWorkspaceId: string | null;
  /** Per-workspace editor tab snapshots, keyed by workspace id. */
  workspaceTabs: Record<string, WorkspaceTabSnapshot>;
  setView: (view: View) => void;
  setActiveArea: (area: ActiveArea) => void;
  setWorkbenchSubTab: (sub: WorkbenchSubTab) => void;
  setTheme: (theme: Theme) => void;
  selectSession: (sessionId: string | null) => void;
  openFile: (path: string) => void;
  closeFile: (path: string) => void;
  setActiveFile: (path: string) => void;
  setRunDrawerTask: (taskId: string | null) => void;
  markDirty: (path: string, dirty: boolean) => void;
  /** Sets the active workspace id (called after IPC activate/add succeeds). */
  setActiveWorkspaceId: (id: string | null) => void;
  /**
   * Saves the current workspace's editor tabs, clears the tab bar, then
   * restores the target workspace's previously saved tabs. If the target
   * has saved tabs the workbench switches to the editor sub-tab; otherwise
   * it stays on (or falls back to) the workspace list.
   */
  switchWorkspace: (workspaceId: string) => void;
}

export const useUiStore = create<UiState>((set) => ({
  view: "chat",
  activeArea: "chat",
  workbenchSubTab: "workspaceList",
  theme: resolveInitialTheme(),
  selectedSessionId: null,
  openFiles: [],
  activeFile: null,
  runDrawerTaskId: null,
  dirtyPaths: {},
  activeWorkspaceId: null,
  workspaceTabs: {},
  /**
   * Switch the view surface. Also releases the main area from the workbench:
   * `activeArea === "workbench"` short-circuits `renderView` in Shell, so a
   * bare view change while a file is open would silently do nothing. The
   * workbench is an *area*, not a view — no caller that sets a view wants
   * to stay in it.
   */
  setView: (view) => set({ view, activeArea: "chat" }),
  setActiveArea: (activeArea) => set({ activeArea }),
  setWorkbenchSubTab: (workbenchSubTab) => set({ workbenchSubTab }),
  // Pure state flip only — DOM class + persistence side effects live in useTheme.
  setTheme: (theme) => set({ theme }),
  selectSession: (sessionId) => set({ selectedSessionId: sessionId, activeArea: "chat" }),
  openFile: (path) =>
    set((s) => ({
      openFiles: s.openFiles.includes(path) ? s.openFiles : [...s.openFiles, path],
      activeFile: path,
      // Opening a workspace file reveals the workbench area with the editor
      // sub-tab (nav rework: the editor is a sub-surface of the workbench).
      activeArea: "workbench",
      workbenchSubTab: "editor",
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
  setActiveWorkspaceId: (id) => set({ activeWorkspaceId: id }),
  switchWorkspace: (workspaceId) =>
    set((s) => {
      const workspaceTabs = { ...s.workspaceTabs };
      // Save the current workspace's editor tab snapshot.
      if (s.activeWorkspaceId) {
        workspaceTabs[s.activeWorkspaceId] = {
          openFiles: s.openFiles,
          activeFile: s.activeFile,
        };
      }
      // Restore the target workspace's saved tabs (or start fresh).
      const snapshot = workspaceTabs[workspaceId];
      const openFiles = snapshot?.openFiles ?? [];
      const activeFile = snapshot?.activeFile ?? null;
      return {
        activeWorkspaceId: workspaceId,
        openFiles,
        activeFile,
        // Always switch to the editor sub-tab so the user sees the file tree
        // after clicking a workspace item, regardless of saved tabs.
        workbenchSubTab: "editor",
        workspaceTabs,
      };
    }),
}));
