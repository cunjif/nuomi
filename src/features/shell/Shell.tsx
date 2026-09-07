import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { Spinner } from "../../components/ui/Spinner";
import { ipc } from "../../lib/ipc/client";
import { ErrorBoundary } from "../../components/ui/ErrorBoundary";
import { useUiStore, type View } from "../../lib/store/uiStore";
import { ApprovalsView } from "../approvals/ApprovalsView";
import { BoardView } from "../board/BoardView";
import { ChatView } from "../chat/ChatView";
import { SchedulerView } from "../scheduler/SchedulerView";
import { SettingsView } from "../settings/SettingsView";
import { TraceView } from "../trace/TraceView";
import { GitView } from "../git/GitView";
import { AreaNav } from "./AreaNav";
import { EditorArea, EditorToolbar } from "./EditorArea";
import { LeftRail } from "./LeftRail";
import { TopBar } from "./TopBar";
import { WorkspaceSetup } from "./WorkspaceSetup";

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
 * - Alt+H toggles the main area between editor and chat (both directions);
 * - Alt+E switches to the editor area (symmetric counterpart).
 * Monaco never registers these chords by default, and MonacoTab adds none —
 * see the note there. preventDefault + stopImmediatePropagation guarantee
 * nothing else on the window even observes the event.
 */
function useGlobalNavShortcuts(): void {
  const handler = useRef<(e: KeyboardEvent) => void>(() => {});
  handler.current = (e: KeyboardEvent): void => {
    if (!e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    if (e.code === "KeyH") {
      // 不可覆盖: toggle editor ↔ chat.
      e.preventDefault();
      e.stopImmediatePropagation();
      const { activeArea, setActiveArea } = useUiStore.getState();
      setActiveArea(activeArea === "editor" ? "chat" : "editor");
    } else if (e.code === "KeyE") {
      // 不可覆盖: focus the editor area.
      e.preventDefault();
      e.stopImmediatePropagation();
      useUiStore.getState().setActiveArea("editor");
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
  useGlobalNavShortcuts();
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
      <TopBar />
      <div className="flex min-h-0 flex-1">
        <LeftRail />
        <div className="flex min-w-0 flex-1 flex-col">
          {/* VSCode-style nav strip: centered tabs, editor workspace actions
          right-aligned (only while the editor area is active). */}
          <AreaNav right={activeArea === "editor" ? <EditorToolbar /> : undefined} />
          <main className="min-h-0 min-w-0 flex-1 overflow-hidden">
            {/* Mutually exclusive surfaces (需求 5): the editor occupies the
            same area as the conversation view; opening a file flips
            activeArea, Alt+H flips it back. */}
            <ErrorBoundary key={activeArea === "editor" ? "editor" : view}>
              {activeArea === "editor" ? <EditorArea /> : renderView(view)}
            </ErrorBoundary>
          </main>
        </div>
      </div>
    </div>
  );
}

function renderView(view: View): ReactNode {
  switch (view) {
    case "chat":
      return <ChatView />;
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
  }
}
