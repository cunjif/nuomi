import { useQuery } from "@tanstack/react-query";

import { ipc } from "./ipc/client";

const STEWARD_EVENTS_KEY = (aggregateId: string | null, kindPrefix: string | null) =>
  ["steward", "events", aggregateId, kindPrefix] as const;

const APP_SNAPSHOT_KEY = ["steward", "appSnapshot"] as const;

export function useStewardEvents(
  aggregateId: string | null = null,
  kindPrefix: string | null = null,
  limit: number = 100,
) {
  return useQuery({
    queryKey: STEWARD_EVENTS_KEY(aggregateId, kindPrefix),
    queryFn: () => ipc.stewardListEvents(aggregateId, kindPrefix, limit),
    refetchInterval: 3000,
    staleTime: 2000,
  });
}

export function useAppStateSnapshot() {
  return useQuery({
    queryKey: APP_SNAPSHOT_KEY,
    queryFn: () => ipc.stewardGetAppSnapshot(),
    staleTime: 10_000,
  });
}
