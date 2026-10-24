/**
 * Merges persisted session history (listEvents) with live deltas from the
 * ADR-0002 session channel into one chat entry list; recovers seq gaps by
 * refetching history.
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { EventDto } from "../../lib/ipc/bindings.gen";
import { ipc } from "../../lib/ipc/client";
import { asRecord, asString, asSources } from "../../lib/events/payload";
import type { SourceEntryDto } from "../../lib/events/types";
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
  quotedEventSeq?: number;
  quotedExcerpt?: string;
  /** Source evidence riding the assistant message payload (spec 5.2). */
  sources?: SourceEntryDto[];
}

/** Live per-agent stream in a group chat (delta carrying `roleName`). */
export interface GroupStream {
  roleId: string;
  roleName: string;
  text: string;
}

/** Speaker picked by the selector for the upcoming utterance. */
export interface ActiveSpeaker {
  roleId?: string;
  roleName?: string;
}

export const STREAM_ENTRY_ID = "__streaming__";

/** Extract the event seq from a message entry id (`m42` → 42; live-buffer
 *  ids `live-m42` resolve to the same seq). Shared by the single-chat
 *  Bubble and the group SpeakerBubble toolbars. */
export function seqFromId(id: string): number | null {
  const m = /^(?:live-)?m(\d+)$/.exec(id);
  return m ? Number(m[1]) : null;
}

export function toEntries(events: EventDto[]): ChatEntry[] {
  const out: ChatEntry[] = [];
  for (const e of events) {
    const p = asRecord(e.payload);
    if (p === null) continue;
    if (e.kind === "message") {
      const quotedEventSeq =
        typeof p.quoted_event_seq === "number" ? p.quoted_event_seq : undefined;
      out.push({
        id: `m${e.seq}`,
        kind: "message",
        role: asString(p.role) === "user" ? "user" : "assistant",
        roleName: asString(p.role_name) || undefined,
        text: asString(p.content),
        quotedEventSeq,
        quotedExcerpt: quotedEventSeq !== undefined ? asString(p.quoted_excerpt) || undefined : undefined,
        sources: asSources(p.sources),
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
  /** Epoch-ms of the most recent session.delta — stream-liveness signal
   *  for the status pill's idle watchdog (CLI stalls). */
  lastDeltaAt: number | null;
  clearLive: () => void;
  /** Optimistic user bubble shown before the persisted round-trip lands. */
  addOptimistic: (text: string) => void;
  clearOptimistic: () => void;
  /** Group-chat live stream of the currently speaking agent (SPEC 5.8.1-4). */
  groupStream: GroupStream | null;
  /** Agent the selector nominated for the next utterance; null on converge. */
  activeSpeaker: ActiveSpeaker | null;
  /** Highest persisted message seq seen live (turn-transition signal). */
  lastMessageSeq: number;
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
  const [groupStream, setGroupStream] = useState<GroupStream | null>(null);
  const [activeSpeaker, setActiveSpeaker] = useState<ActiveSpeaker | null>(null);
  /** Epoch-ms of the most recent session.delta (single + group streams). */
  const [lastDeltaAt, setLastDeltaAt] = useState<number | null>(null);
  /** Highest persisted message seq seen live (drives turn-transition UI). */
  const [lastMessageSeq, setLastMessageSeq] = useState(0);
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
          setLastDeltaAt(Date.now());
          const deltaRoleName = asString(p.roleName);
          if (deltaRoleName !== "") {
            // Group-chat delta (SPEC 5.8.1-4): accumulate per agent; a new
            // speaker resets the buffer so each utterance streams apart.
            const deltaRoleId = asString(p.roleId);
            setGroupStream((prev) =>
              prev !== null && prev.roleId === deltaRoleId
                ? { ...prev, text: prev.text + asString(p.text) }
                : { roleId: deltaRoleId, roleName: deltaRoleName, text: asString(p.text) },
            );
          } else {
            setStreamText((prev) => prev + asString(p.text));
          }
        } else if (ev.type === "session.message") {
          // Persisted authority arriving live. Append as a completed entry
          // so multiple messages in one batch all survive — overwriting
          // streamText would drop earlier ones (e.g. group-chat turns).
          const msgSeq = typeof p.seq === "number" ? p.seq : null;
          if (msgSeq !== null && msgSeq <= lastMessageSeqRef.current) continue;
          if (msgSeq !== null) {
            lastMessageSeqRef.current = msgSeq;
            setLastMessageSeq(msgSeq);
          }
          liveActivityRef.current = true;
          const deltaTo = typeof p.deltaTo === "number" ? p.deltaTo : null;
          if (deltaTo !== null && sessionId !== null) {
            flow.markDeltasApplied(sessionId, deltaTo);
          } else if (sessionId !== null) {
            flow.dropPendingDeltas(sessionId);
          }
          // The durable text takes over from the streaming buffers.
          setStreamText("");
          setGroupStream(null);
          const role = asString(p.role);
          const roleName = asString(p.role_name) || asString(p.roleName) || undefined;
          if (role === "user" || role === "assistant" || (role === "" && roleName !== undefined)) {
            const quotedEventSeq =
              typeof p.quoted_event_seq === "number" ? p.quoted_event_seq : undefined;
            setLiveEntries((prev) => [
              ...prev,
              {
                id: `live-m${msgSeq ?? prev.length}`,
                kind: "message",
                role: role === "" ? "assistant" : role,
                roleName,
                text: asString(p.content),
                quotedEventSeq,
                quotedExcerpt: quotedEventSeq !== undefined ? asString(p.quoted_excerpt) || undefined : undefined,
                sources: asSources(p.sources),
              },
            ]);
          }
        } else if (ev.type === "session.selector_decision") {
          // Group-chat speaker nomination (SPEC 5.8.1-4): who speaks next.
          // The convergence signal ends the discussion — no speaker follows.
          if (p.converged === true) {
            setActiveSpeaker(null);
          } else {
            setActiveSpeaker({
              roleId: asString(p.chosenRoleId) || undefined,
              roleName: asString(p.chosenRoleName) || undefined,
            });
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
        } else if (ev.type === "session.events_rolled_back") {
          // Message rollback: refresh the event list so rolled-back
          // messages disappear from the view.
          if (sessionId !== null) void qc.invalidateQueries({ queryKey: ["sessionEvents", sessionId] });
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
    setGroupStream(null);
    setActiveSpeaker(null);
    setLastMessageSeq(0);
    setLastDeltaAt(null);
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
    // Group buffer included: a retried turn may re-nominate the SAME
    // speaker, and the per-agent accumulator would otherwise splice the
    // fresh utterance onto the interrupted one.
    setGroupStream(null);
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
    lastDeltaAt,
    clearLive,
    addOptimistic,
    clearOptimistic,
    groupStream,
    activeSpeaker,
    lastMessageSeq,
  };
}
