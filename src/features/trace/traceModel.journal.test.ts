/**
 * Harness Journal model derivation: event filtering, entry mapping, drift
 * banner selection and rollback pairing — all pure, no IPC.
 */
import { describe, expect, it } from "vitest";
import type { EventDto, JsonValue } from "../../lib/ipc/bindings.gen";
import {
  buildJournalEntries,
  JOURNAL_ROLLBACK_WIRED,
  JOURNAL_SESSION_ID,
  latestDriftAlert,
  rolledBackFromSeq,
} from "./traceModel";

function event(seq: number, kind: string, payload: JsonValue): EventDto {
  return { seq, kind, payload, createdAt: 1_700_000_000_000 + seq };
}

const appliedPayload = (seq: number) =>
  ({
    seq,
    ts: 1_700_000_000_000 + seq,
    domain: "system_prompt",
    actor: "versioning",
    summary: `applied v${seq}`,
    evidence_refs: ["pv-1"],
    snapshot: { after: { id: "pv-2" } },
  }) as JsonValue;

describe("buildJournalEntries", () => {
  it("keeps only journal.* events and maps the entry shape", () => {
    const entries = buildJournalEntries([
      event(1, "message", { content: "hi" }),
      event(2, "journal.applied", appliedPayload(2)),
      event(3, "journal.drift_alert", {
        seq: 3,
        ts: 1_700_000_000_003,
        domain: "evolution",
        actor: "drift_detector",
        summary: "rate tripped",
        metric: "apply_rollback_rate",
        delta: 0.5,
        evidence_refs: [],
      }),
      event(4, "tool_call", { tool: "x" }),
    ]);
    expect(entries.map((e) => e.kind)).toEqual(["applied", "drift_alert"]);
    expect(entries[0]).toMatchObject({
      seq: 2,
      actor: "versioning",
      domain: "system_prompt",
      summary: "applied v2",
      evidenceRefs: ["pv-1"],
    });
  });

  it("tolerates malformed payloads and unknown kinds", () => {
    const entries = buildJournalEntries([
      event(1, "journal.mystery_kind", "not-an-object"),
      event(2, "journal.applied", null),
    ]);
    expect(entries.map((e) => e.kind)).toEqual(["other", "applied"]);
    const [first, second] = entries;
    expect(first).toMatchObject({ actor: "?", domain: "-", summary: "" });
    // Falls back to the event seq / createdAt when the payload omits them.
    expect(second?.seq).toBe(2);
    expect(second?.ts).toBe(1_700_000_000_002);
  });
});

describe("latestDriftAlert", () => {
  it("returns the most recent drift alert, or null", () => {
    const entries = buildJournalEntries([
      event(1, "journal.applied", appliedPayload(1)),
      event(2, "journal.drift_alert", { seq: 2, metric: "apply_rollback_rate", delta: 0.4 }),
      event(3, "journal.applied", appliedPayload(3)),
    ]);
    expect(latestDriftAlert(entries)?.seq).toBe(2);

    const quiet = buildJournalEntries([event(1, "journal.applied", appliedPayload(1))]);
    expect(latestDriftAlert(quiet)).toBeNull();
  });
});

describe("rolledBackFromSeq + rollback wiring", () => {
  it("extracts from_seq from rolled_back payloads", () => {
    const entries = buildJournalEntries([
      event(1, "journal.applied", appliedPayload(1)),
      event(2, "journal.rolled_back", { seq: 2, rolled_back_seq: 1 }),
      event(3, "journal.applied", appliedPayload(3)),
    ]);
    const [firstApplied, rolledBack] = entries;
    expect(rolledBack).toBeDefined();
    if (rolledBack !== undefined) {
      expect(rolledBackFromSeq(rolledBack)).toBe(1);
    }
    if (firstApplied !== undefined) {
      expect(rolledBackFromSeq(firstApplied)).toBeNull();
    }
  });

  it("enables the rollback button once the backend command is wired", () => {
    // `journal_rollback` is registered in tauri_cmds.rs + commands.rs and the
    // bindings were regenerated — the time-travel button must be live.
    expect(JOURNAL_ROLLBACK_WIRED).toBe(true);
    expect(JOURNAL_SESSION_ID).toBe("journal");
  });
});
