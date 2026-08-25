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
  clearLive: () => void;
} {
  const qc = useQueryClient();
  const historyQuery = useQuery({
    queryKey: ["sessionEvents", sessionId],
    queryFn: () => ipc.listEvents(sessionId ?? "", 0),
    enabled: sessionId !== null,
  });
  const [streamText, setStreamText] = useState("");
  const [liveEntries, setLiveEntries] = useState<ChatEntry[]>([]);
  const liveActivityRef = useRef(false);
  const lastSeqRef = useRef(0);

  useEffect(() => {
    lastSeqRef.current = historyQuery.data?.at(-1)?.seq ?? 0;
  }, [historyQuery.data]);

  const handleBatch = useCallback(
    (batch: DomainEvent[]) => {
      let gap = false;
      for (const ev of batch) {
        if (ev.seq !== undefined && ev.sessionId === sessionId) {
          if (ev.seq > lastSeqRef.current + 1) gap = true;
          else lastSeqRef.current = Math.max(lastSeqRef.current, ev.seq);
        }
        const p = asRecord(ev.payload);
        if (p === null) continue;
        if (ev.type === "session.delta") {
          liveActivityRef.current = true;
          setStreamText((prev) => prev + asString(p.text));
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
        }
      }
      if (gap && sessionId !== null) void qc.invalidateQueries({ queryKey: ["sessionEvents", sessionId] });
    },
    [qc, sessionId],
  );
  useDomainEvents(sessionId !== null ? [sessionChannel(sessionId)] : [], handleBatch);

  const entries = useMemo(() => {
    const history = historyQuery.data !== undefined ? toEntries(historyQuery.data) : [];
    const streaming =
      streamText.length > 0
        ? [{ id: STREAM_ENTRY_ID, kind: "message", role: "assistant", text: streamText } satisfies ChatEntry]
        : [];
    return [...history, ...liveEntries, ...streaming];
  }, [historyQuery.data, liveEntries, streamText]);

  const clearLive = useCallback(() => {
    liveActivityRef.current = false;
    setStreamText("");
    setLiveEntries([]);
  }, []);

  return {
    entries,
    isLoading: historyQuery.isLoading,
    error: historyQuery.error,
    retry: () => void historyQuery.refetch(),
    hasLiveActivity: () => liveActivityRef.current,
    clearLive,
  };
}
