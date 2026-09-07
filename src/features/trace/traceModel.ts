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

// ---------------------------------------------------------------------------
// Harness Journal: evolution audit entries mirrored into the events table
// (see crates/nuomi-core/src/store/repos/journal.rs). Read through the
// existing `list_events` IPC and filtered on the `journal.` kind prefix.
// ---------------------------------------------------------------------------

export const JOURNAL_KIND_PREFIX = "journal.";

/** Synthetic session hosting the journal mirror (see repos/journal.rs). */
export const JOURNAL_SESSION_ID = "journal";

/**
 * Wiring point for the main session: flip to `true` once src-tauri
 * registers the `journal_rollback` command (tauri_cmds.rs + commands.rs +
 * bindings regeneration). Until then the rollback buttons stay disabled.
 */
export const JOURNAL_ROLLBACK_WIRED = true;

export type JournalEntryKind =
  | "reflection_triggered"
  | "proposal_generated"
  | "gate_verdict"
  | "applied"
  | "rolled_back"
  | "drift_alert"
  | "baseline_captured"
  | "other";

export interface JournalEntryVm {
  seq: number;
  ts: number;
  kind: JournalEntryKind;
  domain: string;
  actor: string;
  summary: string;
  evidenceRefs: string[];
  payload: JsonValue;
}

const JOURNAL_KINDS: ReadonlySet<string> = new Set([
  "reflection_triggered",
  "proposal_generated",
  "gate_verdict",
  "applied",
  "rolled_back",
  "drift_alert",
  "baseline_captured",
]);

function journalKindOf(eventKind: string): JournalEntryKind {
  const suffix = eventKind.startsWith(JOURNAL_KIND_PREFIX)
    ? eventKind.slice(JOURNAL_KIND_PREFIX.length)
    : eventKind;
  return (JOURNAL_KINDS.has(suffix) ? suffix : "other") as JournalEntryKind;
}

function evidenceRefsOf(payload: Partial<Record<string, JsonValue>> | null): string[] {
  const raw = payload?.evidence_refs;
  return Array.isArray(raw) ? raw.filter((x): x is string => typeof x === "string") : [];
}

/** Derives journal timeline entries from (session- or journal-scoped) events. */
export function buildJournalEntries(events: EventDto[]): JournalEntryVm[] {
  return events
    .filter((e) => e.kind.startsWith(JOURNAL_KIND_PREFIX))
    .sort((a, b) => a.seq - b.seq)
    .map((e) => {
      const p = asRecord(e.payload);
      return {
        seq: typeof p?.seq === "number" ? p.seq : e.seq,
        ts: typeof p?.ts === "number" ? p.ts : e.createdAt,
        kind: journalKindOf(e.kind),
        domain: asString(p?.domain) || "-",
        actor: asString(p?.actor) || "?",
        summary: asString(p?.summary),
        evidenceRefs: evidenceRefsOf(p),
        payload: e.payload,
      };
    });
}

/** Most recent drift alert, if any — drives the warning banner. */
export function latestDriftAlert(entries: JournalEntryVm[]): JournalEntryVm | null {
  for (let i = entries.length - 1; i >= 0; i--) {
    const entry = entries[i];
    if (entry !== undefined && entry.kind === "drift_alert") return entry;
  }
  return null;
}

/** Seq of the Applied entry a RolledBack entry undoes (`from_seq`). */
export function rolledBackFromSeq(entry: JournalEntryVm): number | null {
  if (entry.kind !== "rolled_back") return null;
  const p = asRecord(entry.payload);
  const from = p?.rolled_back_seq;
  return typeof from === "number" ? from : null;
}

// --- rollback IPC (local declaration; client.ts is owned by another agent) ---

type IpcResult<T> = { status: "ok"; data: T } | { status: "error"; error: { generic: { code: string; message: string } } };

/**
 * Direct invoke wrapper for the not-yet-registered `journal_rollback`
 * command — same unwrap semantics as lib/ipc/client.ts, declared locally
 * because client.ts is outside this feature's file ownership. Called only
 * once `JOURNAL_ROLLBACK_WIRED` is flipped after backend wiring.
 */
export async function invokeJournalRollback(seq: number): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  const result = await invoke<IpcResult<unknown>>("journal_rollback", { seq });
  if (result.status === "error") {
    throw new Error(result.error.generic.message);
  }
}
