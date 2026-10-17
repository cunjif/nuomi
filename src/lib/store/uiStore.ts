/**
 * Ephemeral UI state only (typescript-react rule: never duplicate IPC data
 * here — server data lives in TanStack Query).
 */
import { create } from "zustand";
import { ipc } from "../ipc/client";
import { measureAsync } from "../perf/metrics";

export type View = "chat" | "board" | "trace" | "git" | "approvals" | "scheduler" | "settings" | "plugins" | "steward";

/**
 * Which surface owns the main content area: the conversation view (whatever
 * `view` selects) or the workbench (file editor). Mutually exclusive —
 * opening a workspace file flips to "workbench", Alt+H toggles back to "chat".
 */
export type ActiveArea = "chat" | "workbench";

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

/** Per-workspace editor state bucket (ADR 0017). Keyed by workspace id in
 * `editorByWorkspace`. Each workspace owns an independent set of open files,
 * selection, clipboard, dirtiness, and cross-workspace edit references. */
interface WorkspaceEditorState {
  openFiles: string[];
  activeFile: string | null;
  selectedPaths: string[];
  lastSelectedPath: string | null;
  clipboardPaths: string[];
  clipboardMode: "copy" | "cut" | null;
  dirtyPaths: Record<string, boolean>;
  /** Cross-workspace edit refs (ADR 0017): key = local tab path (relative to
   * this workspace), value = where the file actually lives. Reads/writes go
   * to the source workspace; the local path is only the tab label. */
  crossRefs: Record<string, { sourceWorkspaceId: string; sourcePath: string }>;
}

/** Empty editor bucket for a freshly opened workspace. */
function emptyEditorState(): WorkspaceEditorState {
  return {
    openFiles: [],
    activeFile: null,
    selectedPaths: [],
    lastSelectedPath: null,
    clipboardPaths: [],
    clipboardMode: null,
    dirtyPaths: {},
    crossRefs: {},
  };
}

/** Layout mode for the main content area. */
export type LayoutMode = "single" | "split" | "overview";

interface UiState {
  view: View;
  activeArea: ActiveArea;
  theme: Theme;
  selectedSessionId: string | null;
  /**
   * Open chat session ids in strip order: index 0 is the longest-opened tab,
   * the tail is the most recently opened one. Append-only — activating a tab
   * never reorders (ephemeral UI state, not persisted).
   */
  openSessionIds: string[];
  /** Per-workspace editor state buckets (ADR 0017). Keyed by workspace id. */
  editorByWorkspace: Record<string, WorkspaceEditorState>;
  runDrawerTaskId: string | null;
  /**
   * The single open workspace id (ADR-0021 single-open); null = empty state.
   * Also the pointer for the workbench's editor bucket.
   */
  activeWorkspaceId: string | null;
  /**
   * Current layout mode for the main content area. The `split` write path is
   * retired with ADR-0021 (single-open makes it unreachable); the render
   * branch stays until task 6.8 removes the component.
   */
  layoutMode: LayoutMode;
  /** Conversation list workspace filter ("all" = cross-workspace aggregation). */
  conversationWorkspaceFilter: "all" | string;
  setConversationWorkspaceFilter: (filter: "all" | string) => void;
  setView: (view: View) => void;
  setActiveArea: (area: ActiveArea) => void;
  setTheme: (theme: Theme) => void;
  selectSession: (sessionId: string | null) => void;
  /**
   * Opens a chat tab: if id exists, activate in place (no reorder); else append
   * to the tail of the strip (oldest-opened leftmost) + activate.
   */
  openChatTab: (sessionId: string) => void;
  /** Closes a chat tab: remove from openSessionIds (does not delete backend data). Activates neighbor if closing active. */
  closeChatTab: (sessionId: string) => void;
  /**
   * Activates a chat tab: sets selectedSessionId only. The tab keeps its slot
   * in openSessionIds, so clicking a chip never shuffles the strip.
   */
  activateChatTab: (sessionId: string) => void;
  /** Opens a file in the given workspace's editor bucket (ADR 0017). */
  openFile: (path: string, workspaceId: string) => void;
  /** Closes a file in the given workspace's editor bucket. */
  closeFile: (path: string, workspaceId: string) => void;
  /** Sets the active file within the given workspace's bucket. */
  setActiveFile: (path: string, workspaceId: string) => void;
  setSelectedPaths: (paths: string[], workspaceId: string) => void;
  toggleSelected: (path: string, workspaceId: string) => void;
  selectRange: (from: string, to: string, visiblePaths: string[], workspaceId: string) => void;
  clearSelection: (workspaceId: string) => void;
  setClipboard: (paths: string[], mode: "copy" | "cut", workspaceId: string) => void;
  clearClipboard: (workspaceId: string) => void;
  setRunDrawerTask: (taskId: string | null) => void;
  markDirty: (path: string, dirty: boolean, workspaceId: string) => void;
  /** Records a cross-workspace edit ref (ADR 0017): the tab at `localPath`
   *  in `workspaceId` actually lives at `sourcePath` in `sourceWorkspaceId`. */
  registerCrossRef: (
    workspaceId: string,
    localPath: string,
    sourceWorkspaceId: string,
    sourcePath: string,
  ) => void;
  /** Looks up the cross-workspace ref for a tab, if any. */
  crossRefFor: (workspaceId: string, path: string) => { sourceWorkspaceId: string; sourcePath: string } | null;
  /** Sets the open workspace id (called after IPC open/switch succeeds). */
  setActiveWorkspaceId: (id: string | null) => void;
  /**
   * Flips the local active-workspace pointer. Per-workspace editor state
   * lives in `editorByWorkspace` buckets, so switching only flips
   * `activeWorkspaceId` — each workspace's tabs/selection are independently
   * preserved (ADR 0017).
   */
  switchWorkspace: (workspaceId: string) => void;
  /**
   * Opens a directory under the two-level model (ADR-0021): empty state →
   * opens (or creates) the path's owning workspace; open workspace → the
   * directory joins it and auto-focuses. Calls IPC workspaceAddDirectory.
   */
  openDirectory: (path: string) => Promise<void>;
  /**
   * Switches to another workspace (single-open transition): the backend
   * saves the outgoing snapshot, closes it and opens the target. Editor
   * buckets stay in memory (ADR 0017), so switching back restores state.
   * Calls IPC workspaceSwitch.
   */
  switchToWorkspace: (id: string) => Promise<void>;
  /**
   * Sets the layout mode and persists via IPC. `split` is retired with
   * ADR-0021 — the type excludes it.
   */
  setLayoutMode: (mode: Exclude<LayoutMode, "split">) => Promise<void>;
  /** Syncs the open-workspace mirror from the backend (call on startup). */
  syncOpenWorkspace: () => Promise<void>;
}

export const useUiStore = create<UiState>((set, get) => ({
  view: "chat",
  activeArea: "chat",
  theme: resolveInitialTheme(),
  selectedSessionId: null,
  openSessionIds: [],
  editorByWorkspace: {},
  runDrawerTaskId: null,
  activeWorkspaceId: null,
  layoutMode: "single",
  conversationWorkspaceFilter: "all",
  setConversationWorkspaceFilter: (filter) => set({ conversationWorkspaceFilter: filter }),
  /**
   * Switch the view surface. Also releases the main area from the workbench:
   * `activeArea === "workbench"` short-circuits `renderView` in Shell, so a
   * bare view change while a file is open would silently do nothing. The
   * workbench is an *area*, not a view — no caller that sets a view wants
   * to stay in it.
   */
  setView: (view) => set({ view, activeArea: "chat" }),
  setActiveArea: (activeArea) => set({ activeArea }),
  // Pure state flip only — DOM class + persistence side effects live in useTheme.
  setTheme: (theme) => set({ theme }),
  selectSession: (sessionId) =>
    set((s) => {
      if (sessionId === null) return { selectedSessionId: null, activeArea: "chat" };
      // Already-open session: activate in place, no reorder. Otherwise open a
      // new tab at the tail (see openChatTab for the strip ordering contract).
      const openSessionIds = s.openSessionIds.includes(sessionId)
        ? s.openSessionIds
        : [...s.openSessionIds, sessionId];
      return { selectedSessionId: sessionId, activeArea: "chat", openSessionIds };
    }),
  openChatTab: (sessionId) =>
    set((s) => {
      // Already-open session: activate in place, no reorder. Otherwise open a
      // new tab at the tail.
      const openSessionIds = s.openSessionIds.includes(sessionId)
        ? s.openSessionIds
        : [...s.openSessionIds, sessionId];
      return { openSessionIds, selectedSessionId: sessionId };
    }),
  closeChatTab: (sessionId) =>
    set((s) => {
      const openSessionIds = s.openSessionIds.filter((id) => id !== sessionId);
      if (s.selectedSessionId !== sessionId) return { openSessionIds };
      // Closing active tab: prefer right neighbor, else left, else null.
      const closedIdx = s.openSessionIds.indexOf(sessionId);
      const nextActive = openSessionIds[closedIdx] ?? openSessionIds[closedIdx - 1] ?? null;
      return { openSessionIds, selectedSessionId: nextActive };
    }),
  activateChatTab: (sessionId) =>
    set((s) => {
      if (!s.openSessionIds.includes(sessionId)) return s;
      // Activation never reorders: the chip keeps the slot it was opened in.
      return { selectedSessionId: sessionId };
    }),
  openFile: (path, workspaceId) =>
    set((s) => {
      const bucket = s.editorByWorkspace[workspaceId] ?? emptyEditorState();
      const openFiles = bucket.openFiles.includes(path)
        ? bucket.openFiles
        : [...bucket.openFiles, path];
      return {
        editorByWorkspace: {
          ...s.editorByWorkspace,
          [workspaceId]: { ...bucket, openFiles, activeFile: path },
        },
        // Opening a workspace file reveals the workbench area (nav rework:
        // the editor is the workbench surface).
        activeArea: "workbench",
      };
    }),
  closeFile: (path, workspaceId) =>
    set((s) => {
      const bucket = s.editorByWorkspace[workspaceId];
      if (!bucket) return s;
      const openFiles = bucket.openFiles.filter((p) => p !== path);
      const dirtyPaths = { ...bucket.dirtyPaths };
      delete dirtyPaths[path];
      const activeFile =
        bucket.activeFile === path ? (openFiles.at(-1) ?? null) : bucket.activeFile;
      return {
        editorByWorkspace: {
          ...s.editorByWorkspace,
          [workspaceId]: { ...bucket, openFiles, dirtyPaths, activeFile },
        },
      };
    }),
  setActiveFile: (path, workspaceId) =>
    set((s) => {
      const bucket = s.editorByWorkspace[workspaceId] ?? emptyEditorState();
      return {
        editorByWorkspace: {
          ...s.editorByWorkspace,
          [workspaceId]: { ...bucket, activeFile: path },
        },
      };
    }),
  setSelectedPaths: (paths, workspaceId) =>
    set((s) => {
      const bucket = s.editorByWorkspace[workspaceId] ?? emptyEditorState();
      return {
        editorByWorkspace: {
          ...s.editorByWorkspace,
          [workspaceId]: {
            ...bucket,
            selectedPaths: paths,
            lastSelectedPath: paths.length > 0 ? (paths[paths.length - 1] ?? null) : null,
          },
        },
      };
    }),
  toggleSelected: (path, workspaceId) =>
    set((s) => {
      const bucket = s.editorByWorkspace[workspaceId] ?? emptyEditorState();
      const selectedPaths = bucket.selectedPaths.includes(path)
        ? bucket.selectedPaths.filter((p) => p !== path)
        : [...bucket.selectedPaths, path];
      return {
        editorByWorkspace: {
          ...s.editorByWorkspace,
          [workspaceId]: { ...bucket, selectedPaths, lastSelectedPath: path },
        },
      };
    }),
  selectRange: (from, to, visiblePaths, workspaceId) =>
    set((s) => {
      const bucket = s.editorByWorkspace[workspaceId] ?? emptyEditorState();
      const fromIdx = visiblePaths.indexOf(from);
      const toIdx = visiblePaths.indexOf(to);
      if (fromIdx === -1 || toIdx === -1) {
        return {
          editorByWorkspace: {
            ...s.editorByWorkspace,
            [workspaceId]: { ...bucket, selectedPaths: [to] },
          },
        };
      }
      const [lo, hi] = fromIdx <= toIdx ? [fromIdx, toIdx] : [toIdx, fromIdx];
      return {
        editorByWorkspace: {
          ...s.editorByWorkspace,
          [workspaceId]: { ...bucket, selectedPaths: visiblePaths.slice(lo, hi + 1) },
        },
      };
    }),
  clearSelection: (workspaceId) =>
    set((s) => {
      const bucket = s.editorByWorkspace[workspaceId];
      if (!bucket) return s;
      return {
        editorByWorkspace: {
          ...s.editorByWorkspace,
          [workspaceId]: { ...bucket, selectedPaths: [], lastSelectedPath: null },
        },
      };
    }),
  setClipboard: (paths, mode, workspaceId) =>
    set((s) => {
      const bucket = s.editorByWorkspace[workspaceId] ?? emptyEditorState();
      return {
        editorByWorkspace: {
          ...s.editorByWorkspace,
          [workspaceId]: { ...bucket, clipboardPaths: paths, clipboardMode: mode },
        },
      };
    }),
  clearClipboard: (workspaceId) =>
    set((s) => {
      const bucket = s.editorByWorkspace[workspaceId];
      if (!bucket) return s;
      return {
        editorByWorkspace: {
          ...s.editorByWorkspace,
          [workspaceId]: { ...bucket, clipboardPaths: [], clipboardMode: null },
        },
      };
    }),
  setRunDrawerTask: (taskId) => set({ runDrawerTaskId: taskId }),
  // No-op when the flag already matches so per-keystroke onChange calls don't
  // churn subscribers.
  markDirty: (path, dirty, workspaceId) =>
    set((s) => {
      const bucket = s.editorByWorkspace[workspaceId] ?? emptyEditorState();
      if ((bucket.dirtyPaths[path] ?? false) === dirty) return s;
      const dirtyPaths = { ...bucket.dirtyPaths };
      if (dirty) dirtyPaths[path] = true;
      else delete dirtyPaths[path];
      return {
        editorByWorkspace: {
          ...s.editorByWorkspace,
          [workspaceId]: { ...bucket, dirtyPaths },
        },
      };
    }),
  registerCrossRef: (workspaceId, localPath, sourceWorkspaceId, sourcePath) =>
    set((s) => {
      const bucket = s.editorByWorkspace[workspaceId] ?? emptyEditorState();
      return {
        editorByWorkspace: {
          ...s.editorByWorkspace,
          [workspaceId]: {
            ...bucket,
            crossRefs: {
              ...bucket.crossRefs,
              [localPath]: { sourceWorkspaceId, sourcePath },
            },
          },
        },
      };
    }),
  crossRefFor: (workspaceId, path) => {
    const bucket = get().editorByWorkspace[workspaceId];
    return bucket?.crossRefs[path] ?? null;
  },
  setActiveWorkspaceId: (id) => set({ activeWorkspaceId: id }),
  switchWorkspace: (workspaceId) => set({ activeWorkspaceId: workspaceId }),
  openDirectory: async (path) => {
    const entry = await measureAsync(
      "workspace.add_directory",
      () => ipc.workspaceAddDirectory(path),
      { path },
    );
    set({ activeWorkspaceId: entry.id });
  },
  switchToWorkspace: async (id) => {
    await measureAsync(
      "workspace.switch",
      () => ipc.workspaceSwitch(id),
      { workspaceId: id },
    );
    set({ activeWorkspaceId: id });
  },
  setLayoutMode: async (mode) => {
    set({ layoutMode: mode });
    await ipc.setLayoutSnapshot(mode, null);
  },
  syncOpenWorkspace: async () => {
    const open = await ipc.workspaceGetOpen();
    set({ activeWorkspaceId: open?.id ?? null });
  },
}));
