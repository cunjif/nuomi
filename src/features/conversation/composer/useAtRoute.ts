import { useMemo } from "react";
import type { AgentRefDto } from "../../../lib/ipc/client";

export interface AtRouteResult {
  /** "at_directed" when @ targets are present, "default" otherwise. */
  routeMode: "at_directed" | "default";
  /** Agent IDs extracted from @ markers, mapped to participants. */
  targetAgentIds: string[];
  /** @ markers that couldn't be matched to any participant. */
  unresolved: string[];
}

/**
 * Parse @Agent markers from message text and map them to participant
 * agent IDs. Used to route messages directly to specified agents.
 */
export function useAtRoute(
  text: string,
  participants: AgentRefDto[],
): AtRouteResult {
  return useMemo(() => {
    const markers = text.match(/(?:^|\s)@(\S+)/g) ?? [];
    const names = markers.map((m) => m.trim().slice(1).toLowerCase());

    const targetAgentIds: string[] = [];
    const unresolved: string[] = [];

    for (const name of names) {
      const match = participants.find((p) => p.name.toLowerCase() === name);
      if (match) {
        if (!targetAgentIds.includes(match.id)) targetAgentIds.push(match.id);
      } else {
        if (!unresolved.includes(name)) unresolved.push(name);
      }
    }

    return {
      routeMode: targetAgentIds.length > 0 ? "at_directed" : "default",
      targetAgentIds,
      unresolved,
    };
  }, [text, participants]);
}
