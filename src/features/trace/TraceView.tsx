import type { ReactNode } from "react";
import { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useVirtualizer } from "@tanstack/react-virtual";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { ipc } from "../../lib/ipc/client";
import { sessionChannel } from "../../lib/events/types";
import { useDomainEvents } from "../../lib/events/useDomainEvents";
import { useUiStore } from "../../lib/store/uiStore";
import { HandoffChainView } from "./HandoffChain";
import { JournalView } from "./JournalView";
import { TimelineRow } from "./TimelineRow";
import { WhiteBoardFlow } from "./WhiteBoardFlow";
import { buildHandoffChain, buildTimeline, whiteboardNotes } from "./traceModel";

const VIRTUALIZE_THRESHOLD = 100;

type TraceTab = "trace" | "journal";

/** U12 group-chat trace + Harness Journal: dual-tab audit surface. */
export function TraceView(): ReactNode {
  const { t } = useTranslation();
  const [tab, setTab] = useState<TraceTab>("trace");
  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex gap-1 border-b border-ink-muted/30 px-3 pt-2" role="tablist">
        {(["trace", "journal"] as const).map((key) => (
          <button
            key={key}
            type="button"
            role="tab"
            aria-selected={tab === key}
            onClick={() => setTab(key)}
            className={
              tab === key
                ? "border-b-2 border-ink-accent px-3 py-1.5 text-xs font-semibold text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
                : "border-b-2 border-transparent px-3 py-1.5 text-xs text-ink-muted hover:text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
            }
          >
            {t(key === "trace" ? "trace.tabTrace" : "trace.tabJournal")}
          </button>
        ))}
      </div>
      <div className="min-h-0 flex-1">{tab === "trace" ? <TraceTimeline /> : <JournalView />}</div>
    </div>
  );
}

function TraceTimeline(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const sessionId = useUiStore((s) => s.selectedSessionId);
  const eventsQuery = useQuery({
    queryKey: ["sessionEvents", sessionId],
    queryFn: () => ipc.listEvents(sessionId ?? "", 0),
    enabled: sessionId !== null,
  });

  // Live append: any new session event refreshes the derived models.
  useDomainEvents(sessionId !== null ? [sessionChannel(sessionId)] : [], () => {
    if (sessionId !== null) void qc.invalidateQueries({ queryKey: ["sessionEvents", sessionId] });
  });

  const events = eventsQuery.data ?? [];
  const timeline = useMemo(() => buildTimeline(events), [events]);
  const chain = useMemo(() => buildHandoffChain(events), [events]);
  const notes = useMemo(() => whiteboardNotes(events), [events]);

  if (sessionId === null) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-sm text-ink-muted">{t("trace.noSession")}</div>
    );
  }

  return (
    <div className="flex h-full min-h-0">
      <div className="flex min-w-0 flex-1 flex-col">
        <HandoffChainView chain={chain} />
        <h3 className="px-3 pt-2 text-xs font-semibold uppercase tracking-wide text-ink-muted">
          {t("trace.timelineHeading")}
        </h3>
        <div className="min-h-0 flex-1">
          <AsyncBoundary
            isLoading={eventsQuery.isLoading}
            error={eventsQuery.error}
            isEmpty={timeline.length === 0}
            onRetry={() => void eventsQuery.refetch()}
          >
            <TimelineList entries={timeline} speakerFallback={t("trace.speakerFallback")} />
          </AsyncBoundary>
        </div>
      </div>
      <WhiteBoardFlow notes={notes} />
    </div>
  );
}

function TimelineList({ entries, speakerFallback }: { entries: ReturnType<typeof buildTimeline>; speakerFallback: string }): ReactNode {
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 64,
    overscan: 10,
    enabled: entries.length > VIRTUALIZE_THRESHOLD,
  });
  if (entries.length <= VIRTUALIZE_THRESHOLD) {
    return (
      <div className="overflow-y-auto py-1">
        {entries.map((entry) => (
          <TimelineRow key={entry.seq} entry={entry} speakerFallback={speakerFallback} />
        ))}
      </div>
    );
  }
  return (
    <div ref={scrollRef} className="h-full overflow-y-auto">
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((item) => (
          <div
            key={item.key}
            style={{ position: "absolute", top: 0, left: 0, width: "100%", transform: `translateY(${item.start}px)` }}
          >
            <TimelineRow entry={entries[item.index] as (typeof entries)[number]} speakerFallback={speakerFallback} />
          </div>
        ))}
      </div>
    </div>
  );
}
