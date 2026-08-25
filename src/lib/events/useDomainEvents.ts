/**
 * Single domain-event subscription hook (typescript-react rule: stream via
 * the one domain-event hook; batch high-frequency updates with rAF).
 *
 * `session.delta` events carry a NON-PERSISTENT stream ordinal (see
 * deltaSequencer.ts): this hook dedupes replays and reorders out-of-order
 * deltas per session before they reach the batch handler. Consumers that
 * swap the live buffer for persisted text report back through the returned
 * DeltaFlowControl so already-covered deltas are retired idempotently.
 */
import { useEffect, useMemo, useRef } from "react";
import { subscribe, type Unsubscribe } from "./transport";
import { DeltaSequencer } from "./deltaSequencer";
import type { DomainEvent } from "./types";

export type EventBatchHandler = (batch: DomainEvent[]) => void;

/** Imperative controls over the live-delta bookkeeping kept by this hook. */
export type DeltaFlowControl = {
  /**
   * A persisted message covering deltas ≤ uptoSeq (payload `deltaTo`)
   * replaced the live buffer; buffered deltas in that range are dropped,
   * later buffered ones are still delivered.
   */
  markDeltasApplied: (sessionId: string, uptoSeq: number) => void;
  /** A persisted message without coverage info replaced the live buffer. */
  dropPendingDeltas: (sessionId: string) => void;
  /** The kernel restarted its counter (new/resumed session): forget tracking. */
  resetDeltaTracking: (sessionId?: string) => void;
};

/**
 * Subscribes to every channel in `channels`; incoming events are buffered
 * and flushed to `onBatch` once per animation frame. Re-subscribes only when
 * the channel list (by value) changes; cleans up on unmount.
 */
export function useDomainEvents(channels: string[], onBatch: EventBatchHandler): DeltaFlowControl {
  const channelKey = channels.join("\n");
  const bufferRef = useRef<DomainEvent[]>([]);
  const rafRef = useRef<number | null>(null);
  const handlerRef = useRef(onBatch);
  handlerRef.current = onBatch;
  const enqueueRef = useRef<(e: DomainEvent) => void>(() => {});
  const sequencersRef = useRef<Map<string, DeltaSequencer>>(new Map());

  const flowControl = useMemo<DeltaFlowControl>(
    () => ({
      markDeltasApplied: (sessionId, uptoSeq) => {
        for (const event of sequencersRef.current.get(sessionId)?.markAppliedUpTo(uptoSeq) ?? []) {
          enqueueRef.current(event);
        }
      },
      dropPendingDeltas: (sessionId) => {
        sequencersRef.current.get(sessionId)?.dropSeen();
      },
      resetDeltaTracking: (sessionId) => {
        if (sessionId === undefined) sequencersRef.current.clear();
        else sequencersRef.current.delete(sessionId);
      },
    }),
    [],
  );

  useEffect(() => {
    if (channelKey.length === 0) return;
    let disposed = false;
    const unsubs: Unsubscribe[] = [];

    const flush = () => {
      rafRef.current = null;
      const batch = bufferRef.current;
      bufferRef.current = [];
      if (batch.length > 0) handlerRef.current(batch);
    };
    const enqueue = (e: DomainEvent) => {
      bufferRef.current.push(e);
      if (rafRef.current === null) rafRef.current = requestAnimationFrame(flush);
    };
    enqueueRef.current = enqueue;

    const onEvent = (e: DomainEvent) => {
      // Delta ordinals are live-stream-only: dedupe/reorder per session
      // before delivery. Everything else passes through untouched — its seq
      // is the authoritative persisted one and is handled by consumers.
      if (e.type === "session.delta" && typeof e.sessionId === "string") {
        let sequencer = sequencersRef.current.get(e.sessionId);
        if (!sequencer) {
          sequencer = new DeltaSequencer();
          sequencersRef.current.set(e.sessionId, sequencer);
        }
        for (const ordered of sequencer.accept(e)) enqueue(ordered);
        return;
      }
      enqueue(e);
    };

    for (const channel of channelKey.split("\n")) {
      void subscribe(channel, onEvent).then((unsub) => {
        if (disposed) unsub();
        else unsubs.push(unsub);
      });
    }
    return () => {
      disposed = true;
      enqueueRef.current = () => {};
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
      bufferRef.current = [];
      unsubs.forEach((unsub) => unsub());
    };
  }, [channelKey]);

  return flowControl;
}
