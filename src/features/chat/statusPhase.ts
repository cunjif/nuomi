import { useEffect, useRef, useState } from "react";

// ---------- phase type & copy constants ----------
// Verbatim mechanism from the retired AgentStatusIndicator: time-heuristic
// phases (0-2s instantiating, 2-4s preparing, 4s+ thinking) with rotating
// fun copy per phase. Consumers (ChatStatusPill, group-chat rolling pill)
// feed it isPending/hasFirstToken and may override via backendPhase once
// real backend phase events exist.

export type AgentStatusPhase = "instantiating" | "preparing" | "thinking";

export const STATUS_COPY: Record<AgentStatusPhase, string[]> = {
  instantiating: [
    "chat.status.instantiating.1",
    "chat.status.instantiating.2",
    "chat.status.instantiating.3",
    "chat.status.instantiating.4",
    "chat.status.instantiating.5",
    "chat.status.instantiating.6",
  ],
  preparing: [
    "chat.status.preparing.1",
    "chat.status.preparing.2",
    "chat.status.preparing.3",
    "chat.status.preparing.4",
    "chat.status.preparing.5",
  ],
  thinking: [
    "chat.status.thinking.1",
    "chat.status.thinking.2",
    "chat.status.thinking.3",
    "chat.status.thinking.4",
    "chat.status.thinking.5",
    "chat.status.thinking.6",
    "chat.status.thinking.7",
    "chat.status.thinking.8",
  ],
};

const STATUS_ROTATION_INTERVAL_MS = 4000;

/** Delay before the waiting pill first renders — skips the flash on very
 *  fast responses (spec 5.8.1-1a: the pill appears within 300ms). */
export const STATUS_RENDER_DELAY_MS = 300;

// ---------- phase detection & copy rotation hook ----------

/**
 * Determines the current status phase and rotating copy key based on
 * time heuristics (0-2s instantiating, 2-4s preparing, 4s+ thinking).
 * Backend phase events would override heuristics — the hook accepts
 * an optional `backendPhase` for future wiring.
 */
export function useStatusPhase(
  isPending: boolean,
  hasFirstToken: boolean,
  backendPhase?: AgentStatusPhase | null,
): { phase: AgentStatusPhase; copyKey: string } | null {
  const [phase, setPhase] = useState<AgentStatusPhase>("instantiating");
  const [copyIndex, setCopyIndex] = useState(0);
  const startRef = useRef<number | null>(null);

  // Reset / start tracking when pending begins.
  useEffect(() => {
    if (isPending && !hasFirstToken) {
      if (startRef.current === null) {
        startRef.current = Date.now();
        setPhase("instantiating");
        setCopyIndex(0);
      }
    } else {
      startRef.current = null;
    }
  }, [isPending, hasFirstToken]);

  // Phase progression via time heuristic, overridable by backend.
  useEffect(() => {
    if (startRef.current === null) return;
    const interval = setInterval(() => {
      if (backendPhase) {
        setPhase(backendPhase);
        return;
      }
      const elapsed = Date.now() - (startRef.current ?? Date.now());
      let next: AgentStatusPhase = "instantiating";
      if (elapsed >= 4000) next = "thinking";
      else if (elapsed >= 2000) next = "preparing";
      setPhase((prev) => {
        if (prev !== next) setCopyIndex(0);
        return next;
      });
    }, 500);
    return () => clearInterval(interval);
  }, [backendPhase]);

  // Copy rotation within the current phase.
  useEffect(() => {
    if (startRef.current === null) return;
    const interval = setInterval(() => {
      setCopyIndex((i) => (i + 1) % STATUS_COPY[phase].length);
    }, STATUS_ROTATION_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [phase]);

  if (!isPending || hasFirstToken) return null;
  const keys = STATUS_COPY[phase];
  return { phase, copyKey: keys[copyIndex % keys.length]! };
}
