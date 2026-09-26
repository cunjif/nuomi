import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";

export type ConnectionStatus = "connected" | "disconnected" | "checking";

export interface ConnectionStatusInfo {
  status: ConnectionStatus;
  color: "bg-state-ok" | "bg-state-danger" | "bg-state-warn";
}

/**
 * Shared IPC connection health probe (doubles as the sessions poll seed).
 * Extracted from AreaNav so ChatTabBar can reuse the same signal without
 * duplicate polling.
 */
export function useConnectionStatus(): ConnectionStatusInfo {
  const probe = useQuery({ queryKey: ["sessions"], queryFn: ipc.listSessions, staleTime: 10_000 });
  const status: ConnectionStatus = probe.isPending ? "checking" : probe.isError ? "disconnected" : "connected";
  const color =
    status === "connected" ? "bg-state-ok" : status === "disconnected" ? "bg-state-danger" : "bg-state-warn";
  return { status, color };
}
