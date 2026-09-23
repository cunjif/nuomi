import type { ReactNode } from "react";
import { useEffect } from "react";
import { useStickToBottom } from "../../../components/ui/useStickToBottom";
import type { EventDto } from "../../../lib/ipc/client";
import { ToolCard } from "../../chat/ToolCard";

export interface RunTimelineProps {
  events: EventDto[];
  streamingId?: string | null;
}

/** Tool calls + messages timeline for background runs. */
export function RunTimeline({ events, streamingId }: RunTimelineProps): ReactNode {
  const { scrollRef, scrollToBottomIfStuck } = useStickToBottom();
  const lastSeq = events[events.length - 1]?.seq ?? 0;
  useEffect(() => {
    scrollToBottomIfStuck();
  }, [events.length, lastSeq, streamingId, scrollToBottomIfStuck]);

  return (
    <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto p-2">
      {events.map((e) => {
        if (e.kind === "tool_call") {
          const tool = (e.payload as { tool?: string }).tool ?? "";
          const args = (e.payload as { arguments?: string }).arguments ?? "";
          return <ToolCard key={e.seq} label="Tool call" tool={tool} body={args} />;
        }
        if (e.kind === "message") {
          const role = (e.payload as { role?: string }).role ?? "user";
          const content = (e.payload as { content?: string }).content ?? "";
          const isAssistant = role === "assistant";
          return (
            <div
              key={e.seq}
              className={`mx-3 my-1 max-w-[80%] rounded-lg px-3 py-2 text-sm ${
                isAssistant
                  ? "mr-auto bg-surface-raised"
                  : "ml-auto bg-ink-accent/10"
              }`}
            >
              {streamingId === `entry-${e.seq}` && (
                <span className="mr-1 inline-block animate-pulse">●</span>
              )}
              {content}
            </div>
          );
        }
        return null;
      })}
    </div>
  );
}
