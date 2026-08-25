/**
 * Pure derivation of trace-view models from session events: the speech
 * timeline, the handoff chain (with cycle detection) and whiteboard notes.
 */
import type { EventDto, JsonValue } from "../../lib/ipc/bindings.gen";
import { asRecord, asString } from "../../lib/events/payload";

export const HANDOFF_TOOL = "handoff_to_next";

export type TraceEntry =
  | { seq: number; kind: "speech"; speaker: string; content: string }
  | { seq: number; kind: "tool"; tool: string; argsJson: string }
  | { seq: number; kind: "tool_result"; content: string }
  | { seq: number; kind: "whiteboard"; body: string };

export interface HandoffEdge {
  from: string;
  to: string;
}

export interface HandoffChain {
  edges: HandoffEdge[];
  cycle: boolean;
}

function speakerOf(payload: Partial<Record<string, JsonValue>> | null): string {
  return asString(payload?.role_name) || asString(payload?.role) || "?";
}

export function buildTimeline(events: EventDto[]): TraceEntry[] {
  const sorted = [...events].sort((a, b) => a.seq - b.seq);
  const out: TraceEntry[] = [];
  for (const e of sorted) {
    const p = asRecord(e.payload);
    if (p === null) continue;
    if (e.kind === "message") {
      out.push({ seq: e.seq, kind: "speech", speaker: speakerOf(p), content: asString(p.content) });
    } else if (e.kind === "tool_call") {
      out.push({ seq: e.seq, kind: "tool", tool: asString(p.tool), argsJson: JSON.stringify(p.arguments ?? {}) });
    } else if (e.kind === "tool_result") {
      out.push({ seq: e.seq, kind: "tool_result", content: asString(p.content) });
    } else if (e.kind.includes("whiteboard")) {
      out.push({ seq: e.seq, kind: "whiteboard", body: asString(p.body) || asString(p.content) });
    }
  }
  return out;
}

/** A→B edges from `handoff_to_next` tool calls; flags revisits as a cycle. */
export function buildHandoffChain(events: EventDto[]): HandoffChain {
  const sorted = [...events].sort((a, b) => a.seq - b.seq);
  const edges: HandoffEdge[] = [];
  let lastSpeaker = "";
  let lastSpeakerBeforeHandoff = "";
  const visited = new Set<string>();
  let cycle = false;
  for (const e of sorted) {
    const p = asRecord(e.payload);
    if (p === null) continue;
    if (e.kind === "message") {
      lastSpeaker = speakerOf(p);
      continue;
    }
    if (e.kind === "tool_call" && asString(p.tool) === HANDOFF_TOOL) {
      const args = asRecord(p.arguments);
      const target = asString(args?.target);
      if (target.length === 0) continue;
      lastSpeakerBeforeHandoff = lastSpeaker;
      if (visited.has(target)) cycle = true;
      visited.add(target);
      edges.push({ from: lastSpeakerBeforeHandoff, to: target });
      lastSpeaker = target;
    }
  }
  return { edges, cycle };
}

export function whiteboardNotes(events: EventDto[]): Array<{ seq: number; body: string }> {
  return buildTimeline(events)
    .filter((e): e is Extract<TraceEntry, { kind: "whiteboard" }> => e.kind === "whiteboard")
    .map((e) => ({ seq: e.seq, body: e.body }));
}
