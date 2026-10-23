import { describe, expect, it, vi, afterEach } from "vitest";
import { asRecord, asSources, asString } from "./payload";
import { toEntries } from "../../features/chat/useSessionStream";
import type { EventDto } from "../ipc/bindings.gen";
import type { JsonValue } from "../ipc/bindings.gen";

function messageEvent(seq: number, payload: Record<string, JsonValue>): EventDto {
  return {
    id: seq,
    aggregateType: "session",
    aggregateId: "s1",
    kind: "message",
    payload: payload as JsonValue,
    seq,
    createdAt: 1,
  } as unknown as EventDto;
}

describe("asSources — defensive source guard", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("parses a well-formed sources array", () => {
    const got = asSources([
      { kind: "workspace_file", target: "C:\\a\\b.rs", label: "b.rs", agentId: "r1", agentName: "Res" },
      { kind: "cli_agent", target: "ap-1", label: "Codex", agentId: "ap-1", agentName: "Codex" },
    ]);
    expect(got).not.toBeUndefined();
    expect(got?.length).toBe(2);
    expect(got?.[0]?.kind).toBe("workspace_file");
    expect(got?.[1]?.kind).toBe("cli_agent");
  });

  it("drops malformed entries without throwing", () => {
    const debug = vi.spyOn(console, "debug").mockImplementation(() => {});
    const got = asSources([
      { kind: "bogus", target: "x" },
      { kind: "web_url", target: "" },
      "not-an-object",
      { kind: "web_url", target: "https://ok.example", label: "ok", agentId: "", agentName: "" },
    ]);
    expect(got?.length).toBe(1);
    expect(got?.[0]?.target).toBe("https://ok.example");
    expect(debug).toHaveBeenCalled();
  });

  it("returns undefined for absent/empty/non-array shapes", () => {
    expect(asSources(undefined)).toBeUndefined();
    expect(asSources("nope" as unknown as JsonValue)).toBeUndefined();
    expect(asSources([])).toBeUndefined();
    expect(asSources([{ kind: "bogus", target: "x" }])).toBeUndefined();
  });

  it("toEntries maps message payload sources onto ChatEntry (history path)", () => {
    const entries = toEntries([
      messageEvent(1, {
        role: "assistant",
        content: "answer",
        sources: [{ kind: "web_url", target: "https://a.example", label: "a", agentId: "r1", agentName: "R" }],
      }),
      messageEvent(2, { role: "assistant", content: "legacy" }),
    ]);
    const first = entries[0];
    const second = entries[1];
    expect(first?.sources?.[0]?.target).toBe("https://a.example");
    expect(second?.sources).toBeUndefined();
  });

  it("toEntries keeps existing fields intact alongside sources", () => {
    const entries = toEntries([
      messageEvent(1, { role: "user", content: "q", quoted_event_seq: 1, quoted_excerpt: "ex" }),
    ]);
    expect(asRecord(undefined)).toBeNull();
    expect(asString(undefined)).toBe("");
    expect(entries[0]?.quotedExcerpt).toBe("ex");
  });
});
