import type { ReactNode } from "react";
import { useEffect } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useStickToBottom } from "../../components/ui/useStickToBottom";
import { Bubble } from "./Bubble";
import type { ChatEntry } from "./useSessionStream";

interface MessageListProps {
  entries: ChatEntry[];
  streamingId: string | null;
  /** Called when the user clicks the reply button on a message. */
  onQuote?: (seq: number) => void;
  /** Called when the user clicks a quote preview inside a message. */
  onQuoteClick?: (seq: number) => void;
  /** Called when the user clicks "rollback to here" on a user message. */
  onRollback?: (seq: number) => void;
}

/** Plain list below the threshold; virtualized above it (AC11). */
const VIRTUALIZE_THRESHOLD = 100;

export function MessageList({ entries, streamingId, onQuote, onQuoteClick, onRollback }: MessageListProps): ReactNode {
  const virtualized = entries.length > VIRTUALIZE_THRESHOLD;
  const { scrollRef, scrollToBottomIfStuck } = useStickToBottom({ rebindKey: virtualized });

  // Snap to the latest message whenever the list grows or the streaming
  // bubble updates — but only while the user is following the tail.
  const lastText = entries[entries.length - 1]?.text ?? "";
  useEffect(() => {
    scrollToBottomIfStuck();
  }, [entries.length, lastText, streamingId, virtualized, scrollToBottomIfStuck]);

  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 56,
    overscan: 10,
    enabled: virtualized,
  });

  if (!virtualized) {
    return (
      <div ref={scrollRef} className="min-h-0 h-full overflow-y-auto py-2">
        {entries.map((entry) => (
          <div key={entry.id} data-entry-id={entry.id}>
            <Bubble entry={entry} streaming={entry.id === streamingId} onQuote={onQuote} onQuoteClick={onQuoteClick} onRollback={onRollback} />
          </div>
        ))}
      </div>
    );
  }
  return (
    <div ref={scrollRef} className="min-h-0 h-full overflow-y-auto">
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((item) => (
          <div
            key={entries[item.index]?.id ?? item.key}
            data-entry-id={entries[item.index]?.id}
            style={{
              position: "absolute",
              top: 0,
              left: 0,
              width: "100%",
              transform: `translateY(${item.start}px)`,
            }}
          >
            <Bubble entry={entries[item.index] as ChatEntry} streaming={entries[item.index]?.id === streamingId} onQuote={onQuote} onQuoteClick={onQuoteClick} onRollback={onRollback} />
          </div>
        ))}
      </div>
    </div>
  );
}
