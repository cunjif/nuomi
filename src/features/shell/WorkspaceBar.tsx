import { useEffect, useRef } from "react";
import { useUiStore } from "../../lib/store/uiStore";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import type { WorkspaceEntryDto } from "../../lib/ipc/bindings.gen";

interface UnreadIndicatorDto {
  workspaceId: string;
  count: number;
}

interface OpenSetDto {
  openWorkspaces: { workspaceId: string; openedAt: number; lastFocusedAt: number; isFocused: boolean }[];
  focusedWorkspaceId: string | null;
  pinnedWorkspaceIds: string[];
  unreadIndicators: UnreadIndicatorDto[];
}

export function WorkspaceBar() {
  // Single-open (ADR-0021): the bar lists the open workspace only; the
  // dedicated switch surface lands with task 6.7.
  const { activeWorkspaceId, switchToWorkspace } = useUiStore();

  const { data: workspaces } = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => invoke<WorkspaceEntryDto[]>("list_workspaces"),
  });

  const { data: openSet } = useQuery({
    queryKey: ["open-set"],
    queryFn: () => invoke<OpenSetDto>("get_open_set"),
    refetchInterval: 5000,
  });

  // Drag-drop: accept directory drops to open them under the two-level model
  // (ADR-0021). The backend WorkspaceGuard validates blacklists (system dirs,
  // .nuomi, etc.) — rejected paths surface as IPC errors we silently ignore.
  const dropRef = useRef(false);
  useEffect(() => {
    if (dropRef.current) return;
    if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) return;
    dropRef.current = true;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/webview")
      .then(({ getCurrentWebview }) =>
        getCurrentWebview().onDragDropEvent((event) => {
          if (event.payload.type !== "drop") return;
          for (const path of event.payload.paths) {
            void useUiStore.getState().openDirectory(path)
              .catch(() => { /* blacklisted or invalid — silently ignore */ });
          }
        }),
      )
      .then((fn) => { unlisten = fn; })
      .catch(() => { /* Tauri runtime unavailable — skip drag-drop */ });
    return () => { unlisten?.(); };
  }, []);

  const unreadMap = new Map(
    (openSet?.unreadIndicators ?? []).map((u) => [u.workspaceId, u.count]),
  );

  const openWorkspaces = (workspaces ?? []).filter((w) =>
    w.id === activeWorkspaceId,
  );

  const sorted = [...openWorkspaces].sort((a, b) => {
    const aPinned = a.isPinned ? 1 : 0;
    const bPinned = b.isPinned ? 1 : 0;
    if (aPinned !== bPinned) return bPinned - aPinned;
    return 0;
  });

  if (sorted.length === 0) return null;

  return (
    <div
      className="flex items-center gap-1 px-2 h-9 border-b border-paper-line/30 bg-paper-bg/50 overflow-x-auto"
      style={{ contain: "inline-size" }}
    >
      {sorted.map((ws) => {
        const isFocused = activeWorkspaceId === ws.id;
        const isPinned = ws.isPinned === true;
        const unreadCount = unreadMap.get(ws.id) ?? 0;
        return (
          <button
            key={ws.id}
            onClick={() => void switchToWorkspace(ws.id)}
            style={{ contain: "layout style" }}
            className={`group flex items-center gap-1.5 px-3 py-1 rounded-t-md text-sm transition-colors ${
              isFocused
                ? "bg-paper-bg border-b-2 border-paper-accent"
                : "hover:bg-paper-bg/60"
            }`}
          >
            <span
              className="w-2 h-2 rounded-full shrink-0"
              style={{ backgroundColor: `var(--color-${ws.colorTag})` }}
            />
            <span className="truncate max-w-32">
              {ws.label ?? ws.id}
            </span>
            {isPinned && <span className="text-xs text-paper-muted">★</span>}
            {unreadCount > 0 && (
              <span className="ml-1 px-1.5 py-0.5 text-xs rounded-full bg-paper-accent text-paper-bg">
                {unreadCount}
              </span>
            )}
            {!isPinned && (
              <span
                onClick={(e) => {
                  e.stopPropagation();
                  // Retirement shim (alive for one version cycle): v2 closes
                  // via removing the last directory or switching; the
                  // dedicated surface lands with task 6.7.
                  void invoke("close_workspace", { id: ws.id, force: true })
                    .then(() => useUiStore.getState().syncOpenWorkspace())
                    .catch(() => { /* close failures surface via open-state sync */ });
                }}
                className="ml-1 text-xs text-paper-muted opacity-0 group-hover:opacity-100 hover:text-paper-fg"
              >
                ×
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}
