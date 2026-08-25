/**
 * Single domain-event subscription hook (typescript-react rule: stream via
 * the one domain-event hook; batch high-frequency updates with rAF).
 */
import { useEffect, useRef } from "react";
import { subscribe, type Unsubscribe } from "./transport";
import type { DomainEvent } from "./types";

export type EventBatchHandler = (batch: DomainEvent[]) => void;

/**
 * Subscribes to every channel in `channels`; incoming events are buffered
 * and flushed to `onBatch` once per animation frame. Re-subscribes only when
 * the channel list (by value) changes; cleans up on unmount.
 */
export function useDomainEvents(channels: string[], onBatch: EventBatchHandler): void {
  const channelKey = channels.join("\n");
  const bufferRef = useRef<DomainEvent[]>([]);
  const rafRef = useRef<number | null>(null);
  const handlerRef = useRef(onBatch);
  handlerRef.current = onBatch;

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
    const onEvent = (e: DomainEvent) => {
      bufferRef.current.push(e);
      if (rafRef.current === null) rafRef.current = requestAnimationFrame(flush);
    };

    for (const channel of channelKey.split("\n")) {
      void subscribe(channel, onEvent).then((unsub) => {
        if (disposed) unsub();
        else unsubs.push(unsub);
      });
    }
    return () => {
      disposed = true;
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
      rafRef.current = null;
      bufferRef.current = [];
      unsubs.forEach((unsub) => unsub());
    };
  }, [channelKey]);
}
