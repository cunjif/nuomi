/**
 * Ephemeral UI state only (typescript-react rule: never duplicate IPC data
 * here — server data lives in TanStack Query).
 */
import { create } from "zustand";
import { ipc } from "../ipc/client";
import { measureAsync } from "../perf/metrics";
import type { RunningTaskSummaryDto } from "../ipc/bindings.gen";

export type View = "chat" | "board" | "trace" | "git" | "sandbox" | "approvals" | "scheduler" | "settings" | "plugins" | "steward";

/**
 * Which surface owns the main content area: the conversation view (whatever
 * `view` selects) or the workbench (file editor). Mutually exclusive —
 * opening a workspace file flips to "workbench", Alt+H toggles back to "chat".
 */
export type ActiveArea = "chat" | "workbench" | "terminal";

/**
 * The five hand-drawn themes. `paper-light`/`eink-paper`/`grid-notebook` are
 * the light family (no `.dark` class); `chalkboard-dark`/`high-contrast` are
 * the dark family and toggle `.dark` on <html> so legacy `.dark`-scoped rules
 * still apply.
 */
export type Theme = "paper-light" | "eink-paper" | "grid-notebook" | "chalkboard-dark" | "high-contrast";

/** Stable display/cycle order for the 5 themes (eink-paper: design.md §2). */
export const THEME_ORDER: ReadonlyArray<Theme> = [
  "paper-light",
  "eink-paper",
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
    if (stored === "paper-light" || stored === "eink-paper" || stored === "grid-notebook" || stored === "chalkboard-dark" || stored === "high-contrast") {
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

/** Best-effort active-run probe for the guarded close (backend truth).
 * Any failure resolves to an empty list — the backend's NeedConfirm
 * refusal remains the backstop, so a failed probe just skips the dialog. */
async function previewRunningTasks(workspaceId: string): Promise<RunningTaskSummaryDto[]> {
  try {
    return await ipc.workspaceClosePreview(workspaceId);
  } catch {
    return [];
  }
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

/** Layout mode for the main content area (split retired with ADR-0021). */
export type LayoutMode = "single" | "overview";

/** A terminal instance in the per-workspace terminal tab strip (ADR-0024). */
export interface TerminalInstance {
  id: string;
  title: string;
  createdAt: number;
}

/**
 * A workspace-level action (switch away / close) intercepted by the unsaved
 * edits guard: the action is parked here while the guard dialog asks the
 * user to discard or cancel. `dirtyFiles` is snapshotted at request time so
 * the dialog shows exactly what would be lost. For a switch,
 * `workspaceId` is the workspace being LEFT (the dirty bucket) and
 * `targetWorkspaceId` is the destination; for a close they are the same.
 * `runningTasks` only applies to closes (switching never kills runs) —
 * the backend's active runs with task titles, snapshotted via
 * `workspaceClosePreview`.
 */
export interface PendingWorkspaceAction {
  kind: "switch" | "close";
  workspaceId: string;
  targetWorkspaceId: string;
  /** Workspace label for dialog copy (the workspace carrying the edits). */
  workspaceLabel: string;
  dirtyFiles: string[];
  /** Active runs that keep running in the background after a close. */
  runningTasks: RunningTaskSummaryDto[];
}

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
   * Parked workspace switch/close intercepted by the unsaved-edits guard
   * (non-null → the guard dialog is showing). Ephemeral UI state.
   */
  pendingWorkspaceAction: PendingWorkspaceAction | null;
  /**
   * ADR-0024 Part A-f: when `openDirectory` hits `NoOpenWorkspace`, the path
   * is parked here so the Shell can show a "create workspace" dialog with the
   * directory pre-filled. Null = no create dialog pending.
   */
  pendingCreateWorkspaceDir: string | null;
  /**
   * The single open workspace id (ADR-0021 single-open); null = empty state.
   * Also the pointer for the workbench's editor bucket.
   */
  activeWorkspaceId: string | null;
  /**
   * chat_rich_bind spec 5.3.1-1b — per-workspace last-activated chat tab
   * (memory only, never persisted across restarts). Written on the
   * `activateChatTab` sync path (activation happens inside its owning
   * workspace's context, so the bucket is always correct); read by the
   * ChatTabBar switch effect to restore the remembered tab.
   */
  lastActiveSessionByWorkspace: Record<string, string>;
  /**
   * chat_rich_bind spec 5.2.1-6 / D10 — one-shot temp file tab for opening
   * a file OUTSIDE every mounted work directory. Pure memory state: never
   * written to work_directories or any persistence; destroyed on workspace
   * switch and app exit.
   */
  tempFileTab: { absPath: string; name: string } | null;
  /**
   * ADR-0024 Part B: per-workspace terminal instances (multi-tab). Each
   * workspace owns an independent set of terminals. Keyed by workspace id.
   */
  terminalByWorkspace: Record<string, { terminals: TerminalInstance[]; activeTerminalId: string | null }>;
  /**
   * ADR-0024 Part B: remembers the previous area when toggling to terminal
   * via Alt+`, so Alt+` again switches back. Null = no previous area recorded.
   */
  previousArea: ActiveArea | null;
  /**
   * Current layout mode for the main content area (split retired with
   * ADR-0021 single-open; the type no longer admits "split").
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
  /** Opens the one-shot temp file tab (read-only absolute-path viewer). */
  openTempFile: (absPath: string, name: string) => void;
  /** Closes the temp file tab (no-op when absent). */
  closeTempFile: () => void;
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
   * Opens a directory under the two-level model (ADR-0021 + ADR-0024):
   * open workspace → the directory joins it and auto-focuses; **no open
   * workspace → parks the path in `pendingCreateWorkspaceDir`** so the Shell
   * shows a "create workspace" dialog (the user names the new workspace,
   * optionally pre-filling this directory as its initial directory).
   */
  openDirectory: (path: string) => Promise<void>;
  /**
   * Creates a named workspace (ADR-0024 Part A). Optionally mounts an initial
   * directory. The new workspace is **not** auto-opened — call
   * `switchToWorkspace` afterwards to open it.
   */
  createWorkspace: (name: string, initialPath?: string) => Promise<string>;
  /** Clears the pending-create-workspace prompt (cancel / dismiss). */
  clearPendingCreateWorkspaceDir: () => void;
  /** ADR-0024 Part B: add a terminal to a workspace's bucket. */
  addTerminal: (workspaceId: string, terminal: TerminalInstance) => void;
  /** ADR-0024 Part B: remove a terminal from a workspace's bucket. */
  removeTerminal: (workspaceId: string, terminalId: string) => void;
  /** ADR-0024 Part B: set the active terminal in a workspace's bucket. */
  setActiveTerminal: (workspaceId: string, terminalId: string) => void;
  /**
   * Switches to another workspace (single-open transition): the backend
   * saves the outgoing snapshot, closes it and opens the target. Editor
   * buckets stay in memory (ADR 0017), so switching back restores state.
   * Calls IPC workspaceSwitch.
   */
  switchToWorkspace: (id: string) => Promise<void>;
  /**
   * Guarded switch: same as switchToWorkspace, but when the workspace being
   * LEFT has unsaved editor edits the action is parked in
   * `pendingWorkspaceAction` instead (the guard dialog asks the user to
   * discard or cancel). Returns true when the switch was executed (or is
   * in flight), false when it was parked behind the guard.
   */
  requestSwitchToWorkspace: (id: string, label?: string) => boolean;
  /**
   * Guarded close: same as the direct close, but when the workspace being
   * CLOSED has unsaved editor edits OR active backend runs, the action is
   * parked in `pendingWorkspaceAction` instead. Resolves to true when the
   * close was executed, false when it was parked behind the guard.
   */
  requestCloseWorkspace: (id: string, label?: string) => Promise<boolean>;
  /** Discards the pending action's dirty marks and executes it. */
  confirmPendingWorkspaceAction: () => Promise<void>;
  /** Drops the pending action without executing it. */
  cancelPendingWorkspaceAction: () => void;
  /** Unsaved file paths of one workspace's editor bucket (true entries only). */
  dirtyFilesOf: (workspaceId: string) => string[];
  /**
   * Closes a workspace over IPC (v2 semantics, ADR-0021: drops the open
   * state only — registry row and directories stay) and re-syncs the
   * active-workspace mirror. `force` skips the backend's active-run
   * refusal (the confirm path passes true). Shared by the direct and
   * guarded close paths.
   */
  closeWorkspaceViaIpc: (workspaceId: string, force?: boolean) => Promise<void>;
  /** Sets the layout mode and persists via IPC. */
  setLayoutMode: (mode: LayoutMode) => Promise<void>;
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
  pendingWorkspaceAction: null,
  pendingCreateWorkspaceDir: null,
  activeWorkspaceId: null,
  lastActiveSessionByWorkspace: {},
  tempFileTab: null,
  terminalByWorkspace: {},
  previousArea: null,
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
      // Per-workspace memory rides the same sync path (spec 5.3.1-1b): the
      // activation happens inside its workspace's context, so bucketing by
      // the currently active workspace is naturally correct.
      const lastActiveSessionByWorkspace =
        s.activeWorkspaceId !== null
          ? { ...s.lastActiveSessionByWorkspace, [s.activeWorkspaceId]: sessionId }
          : s.lastActiveSessionByWorkspace;
      return { selectedSessionId: sessionId, lastActiveSessionByWorkspace };
    }),
  openTempFile: (absPath, name) =>
    set({ tempFileTab: { absPath, name }, activeArea: "workbench" }),
  closeTempFile: () => set({ tempFileTab: null }),
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
  setActiveWorkspaceId: (id) => set({ activeWorkspaceId: id, tempFileTab: null }),
  switchWorkspace: (workspaceId) => set({ activeWorkspaceId: workspaceId, tempFileTab: null }),
  openDirectory: async (path) => {
    try {
      const entry = await measureAsync(
        "workspace.add_directory",
        () => ipc.workspaceAddDirectory(path),
        { path },
      );
      set({ activeWorkspaceId: entry.id, tempFileTab: null });
    } catch (e: unknown) {
      // ADR-0024 Part A-d: no open workspace → park the path so the Shell
      // shows a "create workspace" dialog with this directory pre-filled.
      if (e instanceof Error && e.message.includes("no_open_workspace")) {
        set({ pendingCreateWorkspaceDir: path });
        return;
      }
      throw e;
    }
  },
  createWorkspace: async (name, initialPath) => {
    const entry = await measureAsync(
      "workspace.create",
      () => ipc.workspaceCreate(name, initialPath),
      { name },
    );
    return entry.id;
  },
  clearPendingCreateWorkspaceDir: () => set({ pendingCreateWorkspaceDir: null }),
  addTerminal: (workspaceId, terminal) => {
    const bucket = get().terminalByWorkspace[workspaceId] ?? { terminals: [], activeTerminalId: null };
    set({
      terminalByWorkspace: {
        ...get().terminalByWorkspace,
        [workspaceId]: {
          terminals: [...bucket.terminals, terminal],
          activeTerminalId: terminal.id,
        },
      },
    });
  },
  removeTerminal: (workspaceId, terminalId) => {
    const bucket = get().terminalByWorkspace[workspaceId];
    if (!bucket) return;
    const remaining = bucket.terminals.filter((t) => t.id !== terminalId);
    const activeTerminalId =
      bucket.activeTerminalId === terminalId
        ? (remaining[remaining.length - 1]?.id ?? null)
        : bucket.activeTerminalId;
    set({
      terminalByWorkspace: {
        ...get().terminalByWorkspace,
        [workspaceId]: { terminals: remaining, activeTerminalId },
      },
    });
  },
  setActiveTerminal: (workspaceId, terminalId) => {
    const bucket = get().terminalByWorkspace[workspaceId];
    if (!bucket) return;
    set({
      terminalByWorkspace: {
        ...get().terminalByWorkspace,
        [workspaceId]: { ...bucket, activeTerminalId: terminalId },
      },
    });
  },
  switchToWorkspace: async (id) => {
    await measureAsync(
      "workspace.switch",
      () => ipc.workspaceSwitch(id),
      { workspaceId: id },
    );
    set({ activeWorkspaceId: id, tempFileTab: null });
  },
  // Unsaved-edits guard (workspace-level actions): editor dirty state is a
  // FRONTEND ephemeral fact (uiStore buckets — the backend deliberately
  // never persists it, see find_dirty_files in workspace_open_set), so the
  // guard lives here. MonacoTab unmounts on switch/close and reloads from
  // disk, which silently discards unsaved input — the guard makes that an
  // explicit choice. Switch guards the workspace being LEFT; close guards
  // the workspace being CLOSED (defensively also for non-active ones).
  requestSwitchToWorkspace: (id, label) => {
    const active = get().activeWorkspaceId;
    const dirtyFiles = active !== null ? get().dirtyFilesOf(active) : [];
    if (dirtyFiles.length > 0 && active !== null && active !== id) {
      set({
        pendingWorkspaceAction: {
          kind: "switch",
          workspaceId: active,
          targetWorkspaceId: id,
          workspaceLabel: label ?? active,
          dirtyFiles,
          runningTasks: [],
        },
      });
      return false;
    }
    void get().switchToWorkspace(id);
    return true;
  },
  // Guarded close, two probes: unsaved edits are a local ephemeral fact
  // (bucket dirtyPaths — checked synchronously); active runs are backend
  // truth (workspaceClosePreview, checked when the bucket is clean). Either
  // probe non-empty parks the action; the confirm path force-closes.
  requestCloseWorkspace: async (id, label) => {
    const dirtyFiles = get().dirtyFilesOf(id);
    if (dirtyFiles.length > 0) {
      const runningTasks = await previewRunningTasks(id);
      set({
        pendingWorkspaceAction: {
          kind: "close",
          workspaceId: id,
          targetWorkspaceId: id,
          workspaceLabel: label ?? id,
          dirtyFiles,
          runningTasks,
        },
      });
      return false;
    }
    const runningTasks = await previewRunningTasks(id);
    if (runningTasks.length > 0) {
      set({
        pendingWorkspaceAction: {
          kind: "close",
          workspaceId: id,
          targetWorkspaceId: id,
          workspaceLabel: label ?? id,
          dirtyFiles,
          runningTasks,
        },
      });
      return false;
    }
    void get()
      .closeWorkspaceViaIpc(id)
      .catch(() => { /* close failures surface via open-state sync */ });
    return true;
  },
  confirmPendingWorkspaceAction: async () => {
    const pending = get().pendingWorkspaceAction;
    if (pending === null) return;
    // The user explicitly discarded the edits: drop the dirty marks so the
    // stale flags don't mislead a later guard for the same bucket.
    const bucket = get().editorByWorkspace[pending.workspaceId];
    if (bucket !== undefined) {
      set({
        editorByWorkspace: {
          ...get().editorByWorkspace,
          [pending.workspaceId]: { ...bucket, dirtyPaths: {} },
        },
      });
    }
    set({ pendingWorkspaceAction: null });
    if (pending.kind === "switch") {
      await get().switchToWorkspace(pending.targetWorkspaceId);
    } else {
      // Confirmed through the guard: force past the backend's active-run
      // refusal (the runs keep going in the background either way).
      await get().closeWorkspaceViaIpc(pending.workspaceId, true).catch(() => {
        /* close failures surface via open-state sync */
      });
    }
  },
  cancelPendingWorkspaceAction: () => set({ pendingWorkspaceAction: null }),
  dirtyFilesOf: (workspaceId) => {
    const dirtyPaths = get().editorByWorkspace[workspaceId]?.dirtyPaths ?? {};
    return Object.keys(dirtyPaths).filter((p) => dirtyPaths[p] === true);
  },
  closeWorkspaceViaIpc: async (workspaceId, force = false) => {
    await ipc.workspaceClose(workspaceId, force);
    await get().syncOpenWorkspace();
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
