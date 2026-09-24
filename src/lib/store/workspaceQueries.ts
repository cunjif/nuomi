import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../ipc/client";

export const OPEN_SET_KEY = ["workspaces", "openSet"] as const;
export const RECENT_WORKSPACES_KEY = (limit: number) =>
  ["workspaces", "recent", limit] as const;
export const WORKSPACES_LIST_KEY = ["workspaces"] as const;

export function useOpenSet() {
  return useQuery({
    queryKey: OPEN_SET_KEY,
    queryFn: () => ipc.getOpenSet(),
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
    invalidateRecent: (limit?: number) =>
      qc.invalidateQueries({
        queryKey: limit
          ? RECENT_WORKSPACES_KEY(limit)
          : ["workspaces", "recent"],
      }),
    invalidateAll: () => qc.invalidateQueries({ queryKey: ["workspaces"] }),
  };
}
