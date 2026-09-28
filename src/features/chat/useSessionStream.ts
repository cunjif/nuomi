/**
 * Merges persisted session history (listEvents) with live deltas from the
 * ADR-0002 session channel into one chat entry list; recovers seq gaps by
 * refetching history.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { EventDto } from "../../lib/ipc/bindings.gen";
import { ipc } from "../../lib/ipc/client";
import { asRecord, asString } from "../../lib/events/payload";
import { sessionChannel, type DomainEvent } from "../../lib/events/types";
import { useDomainEvents } from "../../lib/events/useDomainEvents";

export interface ChatEntry {
  id: string;
  kind: "message" | "tool_call" | "tool_result";
  role?: "user" | "assistant";
  roleName?: string;
  text?: string;
  tool?: string;
  argsJson?: string;
  content?: string;
}

export const STREAM_ENTRY_ID = "__streaming__";

export function toEntries(events: EventDto[]): ChatEntry[] {
  const out: ChatEntry[] = [];
  for (const e of events) {
    const p = asRecord(e.payload);
    if (p === null) continue;
    if (e.kind === "message") {
      out.push({
        id: `m${e.seq}`,
        kind: "message",
        role: asString(p.role) === "user" ? "user" : "assistant",
        roleName: asString(p.role_name) || undefined,
        text: asString(p.content),
      });
    } else if (e.kind === "tool_call") {
      out.push({
        id: `c${e.seq}`,
        kind: "tool_call",
        tool: asString(p.tool),
        argsJson: JSON.stringify(p.arguments ?? {}),
      });
    } else if (e.kind === "tool_result") {
      out.push({ id: `r${e.seq}`, kind: "tool_result", content: asString(p.content) });
    }
  }
  return out;
}

export function useSessionStream(sessionId: string | null): {
  entries: ChatEntry[];
  isLoading: boolean;
  error: unknown;
  retry: () => void;
  /** Live activity observed since last clear — guards duplicate final text. */
  hasLiveActivity: () => boolean;
  /** Whether any stream text has arrived (first-token indicator). */
  hasStreamText: boolean;
  clearLive: () => void;
  /** Optimistic user bubble shown before the persisted round-trip lands. */
  addOptimistic: (text: string) => void;
  clearOptimistic: () => void;
} {
  const qc = useQueryClient();
  const historyQuery = useQuery({
    queryKey: ["sessionEvents", sessionId],
    queryFn: () => ipc.listEvents(sessionId ?? "", 0),
    enabled: sessionId !== null,
  });
  const [streamText, setStreamText] = useState("");
  const [liveEntries, setLiveEntries] = useState<ChatEntry[]>([]);
  const [optimisticText, setOptimisticText] = useState<string | null>(null);
  const liveActivityRef = useRef(false);
  const lastSeqRef = useRef(0);
  /** Highest persisted message seq already swapped into the live buffer. */
  const lastMessageSeqRef = useRef(0);

  useEffect(() => {
    lastSeqRef.current = historyQuery.data?.at(-1)?.seq ?? 0;
  }, [historyQuery.data]);

  // Stable delegate: the subscription exists before handleBatch is built,
  // but every flush routes to the latest handler.
  const batchDelegateRef = useRef<(batch: DomainEvent[]) => void>(() => {});
  const flow = useDomainEvents(
    sessionId !== null ? [sessionChannel(sessionId)] : [],
    (batch) => batchDelegateRef.current(batch),
  );

  const handleBatch = useCallback(
    (batch: DomainEvent[]) => {
      let gap = false;
      for (const ev of batch) {
        // session.delta carries a NON-PERSISTENT stream ordinal (dedupe and
        // ordering within the live stream only); it must never feed durable
        // gap recovery, which is keyed on events-table seq.
        if (
          ev.seq !== undefined &&
          ev.type !== "session.delta" &&
          ev.sessionId === sessionId
        ) {
          if (ev.seq > lastSeqRef.current + 1) gap = true;
          else lastSeqRef.current = Math.max(lastSeqRef.current, ev.seq);
        }
        const p = asRecord(ev.payload);
        if (p === null) continue;
        if (ev.type === "session.delta") {
          liveActivityRef.current = true;
          setStreamText((prev) => prev + asString(p.text));
        } else if (ev.type === "session.message") {
          // Persisted authority arriving live. Append as a completed entry
          // so multiple messages in one batch all survive — overwriting
          // streamText would drop earlier ones (e.g. group-chat turns).
          const msgSeq = typeof p.seq === "number" ? p.seq : null;
          if (msgSeq !== null && msgSeq <= lastMessageSeqRef.current) continue;
          if (msgSeq !== null) lastMessageSeqRef.current = msgSeq;
          liveActivityRef.current = true;
          const deltaTo = typeof p.deltaTo === "number" ? p.deltaTo : null;
          if (deltaTo !== null && sessionId !== null) {
            flow.markDeltasApplied(sessionId, deltaTo);
          } else if (sessionId !== null) {
            flow.dropPendingDeltas(sessionId);
          }
          // The durable text takes over from the streaming buffer.
          setStreamText("");
          const role = asString(p.role);
          if (role === "user" || role === "assistant") {
            setLiveEntries((prev) => [
              ...prev,
              {
                id: `live-m${msgSeq ?? prev.length}`,
                kind: "message",
                role,
                roleName: asString(p.role_name) || undefined,
                text: asString(p.content),
              },
            ]);
          }
        } else if (ev.type === "tool.call") {
          liveActivityRef.current = true;
          setLiveEntries((prev) => [
            ...prev,
            { id: `live-c${prev.length}`, kind: "tool_call", tool: asString(p.tool), argsJson: JSON.stringify(p.arguments ?? {}) },
          ]);
        } else if (ev.type === "tool.result") {
          setLiveEntries((prev) => [
            ...prev,
            { id: `live-r${prev.length}`, kind: "tool_result", content: asString(p.content) },
          ]);
        } else if (ev.type === "session.turn_end") {
          // ADR 0015: turn finished — refresh the message queue list so the
          // UI reflects any remaining queued items.
          if (sessionId !== null) void qc.invalidateQueries({ queryKey: ["messageQueue", sessionId] });
        } else if (ev.type === "session.queue_error") {
          // P0-3: a queued turn failed — refresh the queue list so the
          // failed entry is removed from the pending list.
          if (sessionId !== null) void qc.invalidateQueries({ queryKey: ["messageQueue", sessionId] });
        }
      }
      if (gap && sessionId !== null) void qc.invalidateQueries({ queryKey: ["sessionEvents", sessionId] });
    },
    [flow, qc, sessionId],
  );
  batchDelegateRef.current = handleBatch;
  useEffect(() => {
    // Every live buffer below belongs to ONE session: the kernel keeps its
    // delta ordinal per session (switching sessions means the counter
    // restarts at 1 — forget our tracking), and the buffers themselves are
    // scoped to the session that produced them. Carrying them over would
    // splice the previously-viewed conversation's messages onto the newly
    // selected one — the panel would look like it re-opened the old session.
    flow.resetDeltaTracking();
    lastMessageSeqRef.current = 0;
    liveActivityRef.current = false;
    setStreamText("");
    setLiveEntries([]);
    setOptimisticText(null);
  }, [flow, sessionId]);

  const entries = useMemo(() => {
    const history = historyQuery.data !== undefined ? toEntries(historyQuery.data) : [];
    const lastHistory = history[history.length - 1];
    // Drop live message entries whose seq is already covered by history —
    // once the refetch lands the durable row, the live copy would duplicate.
    const historyMessageSeqs = new Set<number>();
    for (const e of historyQuery.data ?? []) {
      if (e.kind === "message") historyMessageSeqs.add(e.seq);
    }
    const liveFiltered = liveEntries.filter((e) => {
      const m = /^live-m(\d+)$/.exec(e.id);
      return !(m && historyMessageSeqs.has(Number(m[1])));
    });
    // Optimistic user bubble: hidden once history already contains the same
    // user message (replaces the optimistic entry without a flicker).
    const showOptimistic =
      optimisticText !== null &&
      !(lastHistory?.role === "user" && lastHistory?.text === optimisticText);
    const optimistic =
      showOptimistic
        ? [
            {
              id: "__optimistic__",
              kind: "message",
              role: "user",
              text: optimisticText!,
            } satisfies ChatEntry,
          ]
        : [];
    // Deduplicate: once history contains the same assistant content as the
    // live stream buffer, drop the streaming entry to avoid rendering two
    // identical bubbles.
    const streamingDuplicate =
      streamText.length > 0 &&
      lastHistory?.role === "assistant" &&
      lastHistory?.text === streamText;
    const streaming =
      streamText.length > 0 && !streamingDuplicate
        ? [
            {
              id: STREAM_ENTRY_ID,
              kind: "message",
              role: "assistant",
              text: streamText,
            } satisfies ChatEntry,
          ]
        : [];
    return [...history, ...optimistic, ...liveFiltered, ...streaming];
  }, [historyQuery.data, liveEntries, streamText, optimisticText]);

  const clearLive = useCallback(() => {
    liveActivityRef.current = false;
    setStreamText("");
    setLiveEntries([]);
  }, []);

  const addOptimistic = useCallback((text: string) => setOptimisticText(text), []);
  const clearOptimistic = useCallback(() => setOptimisticText(null), []);

  return {
    entries,
    isLoading: historyQuery.isLoading,
    error: historyQuery.error,
    retry: () => void historyQuery.refetch(),
    hasLiveActivity: () => liveActivityRef.current,
    hasStreamText: streamText.length > 0,
    clearLive,
    addOptimistic,
    clearOptimistic,
  };
}
