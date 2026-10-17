/**
 * Live steward turn stream (design §2.2.2-A1): subscribes to the steward
 * session channel (`event://session/{id}`) and renders ONE in-flight turn.
 *
 * - `steward.delta` increments accumulate into the streaming buffer;
 * - `steward.message` takes over with the authoritative text (the durable
 *   authority lives in the events table — this is the live view only);
 * - `steward.tool_call` / `steward.tool_result` track the active tool;
 * - `steward.llm_failed` marks the turn degraded (spec 5.1.3-1c: the
 *   resend affordance).
 *
 * The turn's authoritative conclusion arrives via the `stewardSendMessage`
 * IPC return value (design §2.2.2-A1: "此返回值为权威结论") — the
 * component commits the final message from it and closes the live turn
 * with `endTurn`.
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { asRecord, asString } from "../../lib/events/payload";
import { sessionChannel, type DomainEvent } from "../../lib/events/types";
import { useDomainEvents } from "../../lib/events/useDomainEvents";

export interface StewardLiveTurn {
  /** The user message of the in-flight turn (optimistic bubble + resend source). */
  userText: string;
  /** Accumulated `steward.delta` increments. */
  streamText: string;
  /** Authoritative text from `steward.message` — replaces the stream buffer. */
  authoritativeText: string | null;
  /** Tool currently executing (`steward.tool_call` → `steward.tool_result`). */
  activeTool: string | null;
  /** `steward.llm_failed` seen — the turn degrades (spec 5.1.3). */
  failed: boolean;
  failedReason: string | null;
}

/** Failure snapshot returned when a turn is closed. */
export interface StewardTurnResult {
  failed: boolean;
  failedReason: string | null;
}

export function useStewardStream(sessionId: string | null): {
  live: StewardLiveTurn | null;
  /** Opens a new live turn (replaces any stale one). */
  beginTurn: (userText: string) => void;
  /**
   * Closes the live turn and returns its failure snapshot (idempotent —
   * later calls return null). Late stream events after the close are
   * dropped.
   */
  endTurn: () => StewardTurnResult | null;
} {
  const [live, setLive] = useState<StewardLiveTurn | null>(null);
  // Ref mirror so `endTurn` can read the current snapshot synchronously
  // (mutation callbacks must not race React state batching).
  const liveRef = useRef<StewardLiveTurn | null>(null);

  const applyLive = useCallback((next: StewardLiveTurn | null) => {
    liveRef.current = next;
    setLive(next);
  }, []);

  const updateLive = useCallback(
    (fn: (prev: StewardLiveTurn | null) => StewardLiveTurn | null) => {
      applyLive(fn(liveRef.current));
    },
    [applyLive],
  );

  // Buffers belong to one session — reset when the session identity changes.
  useEffect(() => {
    liveRef.current = null;
    setLive(null);
  }, [sessionId]);

  // Stable delegate: the subscription exists before handleBatch is built,
  // but every flush routes to the latest handler.
  const batchDelegateRef = useRef<(batch: DomainEvent[]) => void>(() => {});
  useDomainEvents(sessionId !== null ? [sessionChannel(sessionId)] : [], (batch) =>
    batchDelegateRef.current(batch),
  );

  const handleBatch = useCallback(
    (batch: DomainEvent[]) => {
      for (const ev of batch) {
        const p = asRecord(ev.payload);
        if (p === null) continue;
        switch (ev.type) {
          case "steward.delta": {
            const text = asString(p.text);
            updateLive((prev) =>
              prev === null || prev.authoritativeText !== null
                ? prev
                : { ...prev, streamText: prev.streamText + text },
            );
            break;
          }
          case "steward.message": {
            const text = asString(p.text);
            updateLive((prev) => (prev === null ? prev : { ...prev, authoritativeText: text }));
            break;
          }
          case "steward.tool_call": {
            const name = asString(p.name);
            updateLive((prev) => (prev === null ? prev : { ...prev, activeTool: name }));
            break;
          }
          case "steward.tool_result": {
            updateLive((prev) => (prev === null ? prev : { ...prev, activeTool: null }));
            break;
          }
          case "steward.llm_failed": {
            const reason = asString(p.reason) || "unknown";
            updateLive((prev) =>
              prev === null ? prev : { ...prev, failed: true, failedReason: reason },
            );
            break;
          }
          default:
            break;
        }
      }
    },
    [updateLive],
  );
  batchDelegateRef.current = handleBatch;

  const beginTurn = useCallback(
    (userText: string) => {
      applyLive({
        userText,
        streamText: "",
        authoritativeText: null,
        activeTool: null,
        failed: false,
        failedReason: null,
      });
    },
    [applyLive],
  );

  const endTurn = useCallback((): StewardTurnResult | null => {
    const snapshot = liveRef.current;
    applyLive(null);
    if (snapshot === null) return null;
    return { failed: snapshot.failed, failedReason: snapshot.failedReason };
  }, [applyLive]);

  return { live, beginTurn, endTurn };
}
