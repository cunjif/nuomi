import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

// ---------- 3.2: phase type & copy constants ----------

export type AgentStatusPhase = "instantiating" | "preparing" | "thinking";

const STATUS_COPY: Record<AgentStatusPhase, string[]> = {
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
const STATUS_RENDER_DELAY_MS = 300;

// ---------- 3.3: phase detection & copy rotation hook ----------

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

// ---------- 3.4: AgentStatusIndicator component ----------

/**
 * Renders "{avatar} {status copy}" while a RoleAgent is pending and no
 * first token has arrived. Delays 300ms before first render to avoid
 * flicker on very fast responses. Unmounts immediately on first token
 * or error.
 */
export function AgentStatusIndicator({
  agentName,
  agentColor,
  isPending,
  hasFirstToken,
  hasError,
}: {
  agentName: string;
  agentColor: string;
  isPending: boolean;
  hasFirstToken: boolean;
  hasError: boolean;
}): ReactNode {
  const { t } = useTranslation();
  const [visible, setVisible] = useState(false);
  const status = useStatusPhase(isPending, hasFirstToken);

  // 300ms render delay — skip if first token arrives quickly.
  useEffect(() => {
    if (isPending && !hasFirstToken && !hasError) {
      const timer = setTimeout(() => setVisible(true), STATUS_RENDER_DELAY_MS);
      return () => clearTimeout(timer);
    }
    setVisible(false);
  }, [isPending, hasFirstToken, hasError]);

  if (!visible || !status || hasError) return null;

  const initial = agentName.charAt(0).toUpperCase();

  return (
    <div className="flex items-center gap-2 px-3 py-1.5 text-sm text-ink-muted">
      <span
        className="flex h-6 w-6 shrink-0 items-center justify-center rounded text-xs font-semibold text-surface"
        style={{ backgroundColor: agentColor }}
        aria-label={agentName}
      >
        {initial}
      </span>
      <span className="animate-pulse">{t(status.copyKey)}</span>
    </div>
  );
}
