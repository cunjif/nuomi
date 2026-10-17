import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../ipc/client";

export const OPEN_SET_KEY = ["workspaces", "openSet"] as const;
/** The single open workspace with its work directories (ADR-0021). */
export const OPEN_WORKSPACE_KEY = ["workspaces", "open"] as const;
export const RECENT_WORKSPACES_KEY = (limit: number) =>
  ["workspaces", "recent", limit] as const;
export const WORKSPACES_LIST_KEY = ["workspaces"] as const;

export function useOpenSet() {
  return useQuery({
    queryKey: OPEN_SET_KEY,
    queryFn: () => ipc.getOpenSet(),
  });
}

/**
 * The currently open workspace (two-level model, ADR-0021): entry with
 * derived label and work directories (mounted/available/isActive markers),
 * or null in the empty state. Invalidate after add/remove/mount/focus.
 */
export function useOpenWorkspace() {
  return useQuery({
    queryKey: OPEN_WORKSPACE_KEY,
    queryFn: () => ipc.workspaceGetOpen(),
  });
}

export function useRecentWorkspaces(limit: number) {
  return useQuery({
    queryKey: RECENT_WORKSPACES_KEY(limit),
    queryFn: () => ipc.getRecentWorkspaces(limit),
  });
}

export function useInvalidateWorkspaceQueries() {
  const qc = useQueryClient();
  return {
    invalidateOpenSet: () => qc.invalidateQueries({ queryKey: OPEN_SET_KEY }),
    invalidateOpenWorkspace: () =>
      qc.invalidateQueries({ queryKey: OPEN_WORKSPACE_KEY }),
    invalidateRecent: (limit?: number) =>
      qc.invalidateQueries({
        queryKey: limit
          ? RECENT_WORKSPACES_KEY(limit)
          : ["workspaces", "recent"],
      }),
    invalidateAll: () => qc.invalidateQueries({ queryKey: ["workspaces"] }),
  };
}
