import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Spinner } from "../../components/ui/Spinner";
import { ipc } from "../../lib/ipc/client";
import { invoke } from "@tauri-apps/api/core";
import { ErrorBoundary } from "../../components/ui/ErrorBoundary";
import { useUiStore, type View, type LayoutMode } from "../../lib/store/uiStore";
import { ApprovalsView } from "../approvals/ApprovalsView";
import { BoardView } from "../board/BoardView";
import { ConversationView } from "../conversation/ConversationView";
import { SchedulerView } from "../scheduler/SchedulerView";
import { SettingsView } from "../settings/SettingsView";
import { TraceView } from "../trace/TraceView";
import { GitView } from "../git/GitView";
import { PluginsView } from "../plugins/PluginsView";
import { AreaNav } from "./AreaNav";
import { ChatTabBar } from "./ChatTabBar";
import { isChatPanelActive } from "./isChatPanelActive";
import { BackgroundTray } from "./BackgroundTray";
import { WorkbenchArea } from "./WorkbenchArea";
import { hydratePluginEditorExtensions } from "../../lib/editor-ext/pluginBridge";
import { LeftRail } from "./LeftRail";
import { QuickOpen, useGlobalPaletteShortcuts } from "./QuickOpen";
import { WorkspaceSetup } from "./WorkspaceSetup";
import { SplitView } from "./SplitView";
import { OverviewGrid } from "./OverviewGrid";
import { EmptyStateGuide } from "./EmptyStateGuide";

type WorkspaceInfo = { root: string; configured: boolean };

/** localStorage key for the last successfully validated workspace info. */
const WORKSPACE_CACHE_KEY = "nuomi.workspace.cache";

/** Best-effort read of the cached workspace info (stale-while-revalidate). */
function readCachedWorkspace(): WorkspaceInfo | null {
  try {
    const raw = window.localStorage.getItem(WORKSPACE_CACHE_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (
      parsed !== null &&
      typeof parsed === "object" &&
      typeof (parsed as WorkspaceInfo).root === "string" &&
      typeof (parsed as WorkspaceInfo).configured === "boolean"
    ) {
      return parsed as WorkspaceInfo;
    }
    return null;
  } catch {
    // Corrupted cache entries are treated as absent.
    return null;
  }
}

/** True inside a real Tauri webview (events available); false under vitest. */
function hasTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * Boot screen shown while the Rust kernel is still booting off the
 * window-critical path (the window paints before the kernel is ready, so
 * this screen is what users see first instead of a frozen blank window).
 */
function BootScreen({ error }: { error?: string }): ReactNode {
  const { t } = useTranslation();
  return (
    <div className="flex h-screen flex-col items-center justify-center gap-3 bg-surface text-ink">
      <span className="text-sm font-medium text-ink-muted">{t("shell.appName")}</span>
      {error ? (
        <p role="alert" className="max-w-md px-4 text-center text-xs text-state-danger">
          {t("common.loadFailed")}: {error}
        </p>
      ) : (
        <Spinner />
      )}
    </div>
  );
}

/**
 * Global navigation chords (需求 5), handled at window keydown CAPTURE phase
 * so no inner surface (Monaco keybindings, inputs, composer) can see or
 * override them. 不可覆盖 constraint:
 * - Alt+H toggles the main area between workbench and chat (both directions);
 * - Alt+E switches to the workbench area with the editor sub-tab.
 * Monaco never registers these chords by default, and MonacoTab adds none —
 * see the note there. preventDefault + stopImmediatePropagation guarantee
 * nothing else on the window even observes the event.
 */
function useGlobalNavShortcuts(): void {
  const handler = useRef<(e: KeyboardEvent) => void>(() => {});
  handler.current = (e: KeyboardEvent): void => {
    // Ctrl+Tab / Ctrl+Shift+Tab: cycle chat tabs (multi_chat_tabs).
    if (e.ctrlKey && !e.altKey && !e.metaKey && e.code === "Tab") {
      e.preventDefault();
      e.stopImmediatePropagation();
      const { openSessionIds, activateChatTab } = useUiStore.getState();
      if (openSessionIds.length < 2) return;
      const selected = useUiStore.getState().selectedSessionId;
      const idx = selected ? openSessionIds.indexOf(selected) : -1;
      const next = e.shiftKey
        ? (idx <= 0 ? openSessionIds.length - 1 : idx - 1)
        : (idx < 0 || idx >= openSessionIds.length - 1 ? 0 : idx + 1);
      const target = openSessionIds[next];
      if (target) activateChatTab(target);
      return;
    }
    // Ctrl+Alt+Tab / Ctrl+Alt+Shift+Tab: cycle focused workspace (migrated from Ctrl+Tab).
    if (e.ctrlKey && e.altKey && !e.metaKey && e.code === "Tab") {
      e.preventDefault();
      e.stopImmediatePropagation();
      const { openWorkspaceIds, focusWorkspace } = useUiStore.getState();
      if (openWorkspaceIds.length < 2) return;
      const focused = useUiStore.getState().focusedWorkspaceId;
      const idx = focused ? openWorkspaceIds.indexOf(focused) : -1;
      const next = e.shiftKey
        ? (idx <= 0 ? openWorkspaceIds.length - 1 : idx - 1)
        : (idx < 0 || idx >= openWorkspaceIds.length - 1 ? 0 : idx + 1);
      const target = openWorkspaceIds[next];
      if (target) void focusWorkspace(target);
      return;
    }
    // Alt+1..Alt+8: focus the Nth open workspace (task 8.1).
    if (e.altKey && !e.ctrlKey && !e.metaKey && !e.shiftKey) {
      const digit = e.code.match(/^Digit([1-8])$/);
      if (digit) {
        e.preventDefault();
        e.stopImmediatePropagation();
        const { openWorkspaceIds, focusWorkspace } = useUiStore.getState();
        const n = parseInt(digit[1] ?? "0", 10) - 1;
        if (n < openWorkspaceIds.length) {
          const target = openWorkspaceIds[n];
          if (target) void focusWorkspace(target);
        }
        return;
      }
      if (e.code === "KeyH") {
        e.preventDefault();
        e.stopImmediatePropagation();
        const { activeArea, setActiveArea } = useUiStore.getState();
        setActiveArea(activeArea === "workbench" ? "chat" : "workbench");
      } else if (e.code === "KeyE") {
        e.preventDefault();
        e.stopImmediatePropagation();
        useUiStore.getState().setActiveArea("workbench");
      }
    }
  };
  useEffect(() => {
    const listener = (e: KeyboardEvent): void => handler.current(e);
    window.addEventListener("keydown", listener, true);
    return () => window.removeEventListener("keydown", listener, true);
  }, []);
}

/** U8 shell: top status bar, left rail, area nav, mutually exclusive main area. */
export function Shell(): ReactNode {
  const view = useUiStore((s) => s.view);
  const activeArea = useUiStore((s) => s.activeArea);
  const openWorkspaceIds = useUiStore((s) => s.openWorkspaceIds);
  const layoutMode = useUiStore((s) => s.layoutMode);
  useGlobalNavShortcuts();
  useGlobalPaletteShortcuts();
  const queryClient = useQueryClient();
  // Kernel boot lifecycle as observed through events; the workspace query
  // poll below is the fallback signal when an event is missed.
  const [bootFailed, setBootFailed] = useState<string | undefined>(undefined);
  const [kernelReady, setKernelReady] = useState(false);

  useEffect(() => {
    if (!hasTauriRuntime()) return undefined;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void import("@tauri-apps/api/event")
      .then(({ listen }) =>
        Promise.all([
          listen("kernel-ready", () => {
            setKernelReady(true);
            // Unblock the gated workspace query immediately instead of
            // waiting out the current retry backoff.
            void queryClient.invalidateQueries({ queryKey: ["workspace"] });
            // Sync the frontend open-set mirror from the backend (which has
            // already restored the layout snapshot during boot).
            void useUiStore.getState().syncFromOpenSet();
          }),
          listen<string>("kernel-failed", (e) => {
            setBootFailed(e.payload);
          }),
        ]),
      )
      .then((fns) => {
        if (cancelled) fns.forEach((fn) => fn());
        else unlisten = () => fns.forEach((fn) => fn());
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [queryClient]);

  // Plugin-delivered editor extensions (ADR 0010): hydrate once per mount —
  // the plugin list is a pure disk scan, so this works before kernel-ready;
  // editor RPC calls degrade gracefully until the kernel is up.
  useEffect(() => {
    void hydratePluginEditorExtensions();
  }, []);

  // Drag-drop workspace registration (migrated from WorkspaceBar): accept
  // directory drops to register + open a workspace. The backend WorkspaceGuard
  // validates blacklists — rejected paths surface as IPC errors (silently ignored).
  const dropRef = useRef(false);
  useEffect(() => {
    if (dropRef.current) return;
    if (!hasTauriRuntime()) return;
    dropRef.current = true;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((event) => {
          if (event.payload.type !== "drop") return;
          for (const path of event.payload.paths) {
            void invoke<{ id: string }>("add_workspace", { path })
              .then((entry) => void useUiStore.getState().openWorkspace(entry.id))
              .catch(() => { /* blacklisted or invalid — silently ignore */ });
          }
        }),
      )
      .then((fn) => { unlisten = fn; })
      .catch(() => { /* Tauri runtime unavailable — skip drag-drop */ });
    return () => { unlisten?.(); };
  }, []);

  const workspaceQuery = useQuery({
    queryKey: ["workspace"],
    queryFn: async () => {
      const data = await ipc.getWorkspace();
      try {
        window.localStorage.setItem(WORKSPACE_CACHE_KEY, JSON.stringify(data));
      } catch {
        // Cache is best-effort; storage failures never block rendering.
      }
      return data;
    },
    // While the kernel boots asynchronously, invokes fail ("state not
    // managed"): poll until it succeeds instead of surfacing the error.
    // This doubles as the readiness probe when the kernel-ready event
    // fires before the webview listener exists.
    retry: () => !bootFailed,
    retryDelay: 500,
    staleTime: 30_000,
    placeholderData: readCachedWorkspace() ?? undefined,
  });

  // First launch: no persisted workspace root and no env pin — force setup
  // before any view that depends on the sandbox root.
  if (bootFailed !== undefined) {
    return <BootScreen error={bootFailed} />;
  }
  // Nothing IPC-dependent may render before the kernel is ready (either the
  // event arrived or the workspace poll succeeded).
  if (!kernelReady && !workspaceQuery.isSuccess) {
    return <BootScreen />;
  }
  // Progressive render: with a cached configured workspace the main layout
  // paints immediately on kernel-ready and the query validates in the
  // background (a stale `configured=false` cache just falls through to
  // WorkspaceSetup once validation lands).
  // A not-configured workspace (validated or cached) always routes to setup.
  if (workspaceQuery.data && !workspaceQuery.data.configured) {
    return <WorkspaceSetup />;
  }

  return (
    <div className="flex h-screen flex-col bg-surface text-ink">
      <AreaNav />
      <BackgroundTray />
      <div className="flex min-h-0 flex-1">
        <LeftRail />
        <main className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-surface-raised">
          {/* Strip row: only shown on the chat panel so the tab bar never
              leaks into settings / git / trace / board / etc. views. */}
          {isChatPanelActive(view, activeArea) && (
            <div className="flex shrink-0 items-end border-r border-ink-muted/30 px-3 pt-1.5">
              <ChatTabBar />
            </div>
          )}
          {/* Tab panel: raised card with a small bottom inset. No top border
              — the tab chips and the panel's header (workspace path row) read
              as one continuous surface, so nothing separates them. */}
          <div className="mb-2 min-h-0 flex-1 overflow-hidden rounded-b-lg border-r border-b border-ink-muted/30 bg-surface-raised">
            <ErrorBoundary key={activeArea === "workbench" ? "workbench" : view}>
              {renderMainContent({ activeArea, view, openWorkspaceIds, layoutMode })}
            </ErrorBoundary>
          </div>
        </main>
      </div>
      {/* Global palette overlay (Ctrl+P / Ctrl+Shift+P / Ctrl+F). */}
      <QuickOpen />
    </div>
  );
}

/**
 * Dispatches the main content area based on the multi-workspace layout state
 * (task 6.7.1). Precedence:
 * 1. layoutMode "overview" → OverviewGrid (or EmptyStateGuide if no open ws)
 * 2. layoutMode "split" → SplitView (or EmptyStateGuide if no open ws)
 * 3. layoutMode "single" (default) → the existing mutually-exclusive surface
 *    (WorkbenchArea or the active view) — preserves legacy single-workspace behavior
 */
function renderMainContent({
  activeArea,
  view,
  openWorkspaceIds,
  layoutMode,
}: {
  activeArea: string;
  view: View;
  openWorkspaceIds: string[];
  layoutMode: LayoutMode;
}): ReactNode {
  if (layoutMode === "overview") {
    return openWorkspaceIds.length === 0 ? <EmptyStateGuide /> : <OverviewGrid />;
  }
  if (layoutMode === "split") {
    if (openWorkspaceIds.length === 0) return <EmptyStateGuide />;
    return (
      <SplitView
        renderWorkspace={() =>
          activeArea === "workbench" ? <WorkbenchArea /> : renderView(view)
        }
      />
    );
  }
  // layoutMode === "single"
  return activeArea === "workbench" ? <WorkbenchArea /> : renderView(view);
}

function renderView(view: View): ReactNode {
  switch (view) {
    case "chat":
      return <ConversationView />;
    case "board":
      return <BoardView />;
    case "trace":
      return <TraceView />;
    case "git":
      return <GitView />;
    case "approvals":
      return <ApprovalsView />;
    case "scheduler":
      return <SchedulerView />;
    case "settings":
      return <SettingsView />;
    case "plugins":
      return <PluginsView />;
  }
}
