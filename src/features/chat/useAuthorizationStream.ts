/**
 * Authorization stream (chat_rich_bind spec 5.4.1-3/4, 5.4.3-4): merges the
 * durable `listPendingAuthorizations` query (rebuilt on session open and
 * after app restart — pending rows persist) with a 5s refetch cadence that
 * mirrors the backend sweeper. The timeout-reminder UI state derives from
 * `now` vs. `expiresAt` on still-pending rows (the row state never becomes
 * "waiting_notified" — that is the derived reminder, design §2.3.2).
 */
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";

export interface AuthorizationRequest {
  authorizationId: string;
  sessionId: string;
  roleId: string;
  roleDisplayName: string;
  op: "write" | "delete";
  targetPath: string;
  state:
    | "pending"
    | "granted_once"
    | "granted_session"
    | "denied"
    | "timeout_default"
    | "voided";
  requestedAt: number;
  expiresAt: number;
}

export function useAuthorizationStream(sessionId: string | null): {
  requests: AuthorizationRequest[];
  isLoading: boolean;
  refresh: () => void;
} {
  const qc = useQueryClient();
  const query = useQuery({
    queryKey: ["authorizations", sessionId],
    queryFn: () => ipc.listPendingAuthorizations(sessionId ?? ""),
    enabled: sessionId !== null,
    refetchInterval: 5_000,
  });
  const refresh = (): void => {
    void qc.invalidateQueries({ queryKey: ["authorizations", sessionId] });
  };
  return {
    requests: (query.data ?? []) as AuthorizationRequest[],
    isLoading: query.isLoading,
    refresh,
  };
}
