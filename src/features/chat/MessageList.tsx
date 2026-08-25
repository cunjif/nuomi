import type { ReactNode } from "react";
import { useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Bubble } from "./Bubble";
import type { ChatEntry } from "./useSessionStream";

interface MessageListProps {
  entries: ChatEntry[];
  streamingId: string | null;
}

/** Plain list below the threshold; virtualized above it (AC11). */
const VIRTUALIZE_THRESHOLD = 100;

export function MessageList({ entries, streamingId }: MessageListProps): ReactNode {
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 56,
    overscan: 10,
    enabled: entries.length > VIRTUALIZE_THRESHOLD,
  });

  if (entries.length <= VIRTUALIZE_THRESHOLD) {
    return (
      <div className="min-h-0 flex-1 overflow-y-auto py-2">
        {entries.map((entry) => (
          <Bubble key={entry.id} entry={entry} streaming={entry.id === streamingId} />
        ))}
      </div>
    );
  }
  return (
    <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto">
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((item) => (
          <div
            key={entries[item.index]?.id ?? item.key}
            style={{
              position: "absolute",
              top: 0,
              left: 0,
              width: "100%",
              transform: `translateY(${item.start}px)`,
            }}
          >
            <Bubble entry={entries[item.index] as ChatEntry} streaming={entries[item.index]?.id === streamingId} />
          </div>
        ))}
      </div>
    </div>
  );
}
