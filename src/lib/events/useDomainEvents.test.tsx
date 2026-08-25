import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { useState, type ReactNode } from "react";
import { emitTestEvent, resetTestBus } from "./transport";
import { sessionChannel } from "./types";
import { useDomainEvents } from "./useDomainEvents";

function Probe({ channels }: { channels: string[] }): ReactNode {
  const [seen, setSeen] = useState<string[]>([]);
  useDomainEvents(channels, (batch) => {
    setSeen((prev) => [...prev, ...batch.map((e) => e.type)]);
  });
  return (
    <div>
      <span data-testid="count">{seen.length}</span>
      <span data-testid="types">{seen.join(",")}</span>
    </div>
  );
}

const SID = "s1";
const CH = sessionChannel(SID);

function delta(seq: number | undefined, text: string) {
  return {
    type: "session.delta",
    sessionId: SID,
    ...(seq === undefined ? {} : { seq }),
    payload: { sessionId: SID, text, ...(seq === undefined ? {} : { seq }) },
  };
}

/** Renders delivered delta texts in arrival order + exposes flow control. */
function DeltaProbe({ channels }: { channels: string[] }): ReactNode {
  const [texts, setTexts] = useState<string[]>([]);
  const flow = useDomainEvents(channels, (batch) => {
    setTexts((prev) => [
      ...prev,
      ...batch
        .filter((e) => e.type === "session.delta")
        .map((e) => String((e.payload as { text?: unknown }).text ?? "")),
    ]);
  });
  return (
    <div>
      <span data-testid="texts">{texts.join("|")}</span>
      <button type="button" data-testid="mark" onClick={() => flow.markDeltasApplied(SID, 3)}>
        mark
      </button>
      <button type="button" data-testid="drop" onClick={() => flow.dropPendingDeltas(SID)}>
        drop
      </button>
      <button type="button" data-testid="reset" onClick={() => flow.resetDeltaTracking(SID)}>
        reset
      </button>
    </div>
  );
}

function currentTexts(): string[] {
  const raw = screen.getByTestId("texts").textContent ?? "";
  return raw === "" ? [] : raw.split("|");
}

/** Waits until the rendered delta sequence equals `want`. */
async function expectTexts(want: string[]): Promise<void> {
  await waitFor(() => expect(currentTexts()).toEqual(want));
}

describe("useDomainEvents", () => {
  afterEach(() => {
    cleanup();
    resetTestBus();
  });

  it("receives events on subscribed channels", async () => {
    render(<Probe channels={["event://test/a"]} />);
    expect(screen.getByTestId("count")).toHaveTextContent("0");
    act(() => {
      emitTestEvent("event://test/a", { type: "e1", payload: {} });
    });
    await screen.findByText("e1", { selector: "[data-testid='types']" });
    expect(screen.getByTestId("count")).toHaveTextContent("1");
  });

  it("ignores events on other channels", async () => {
    render(<Probe channels={["event://test/a"]} />);
    act(() => {
      emitTestEvent("event://test/b", { type: "noise", payload: {} });
    });
    await new Promise((r) => setTimeout(r, 30));
    expect(screen.getByTestId("count")).toHaveTextContent("0");
  });

  it("stops receiving after unmount (cleanup)", async () => {
    const { unmount } = render(<Probe channels={["event://test/c"]} />);
    unmount();
    act(() => {
      emitTestEvent("event://test/c", { type: "late", payload: {} });
    });
    await new Promise((r) => setTimeout(r, 30));
    expect(screen.queryByTestId("count")).not.toBeInTheDocument();
  });

  it("batches a burst of events into one flush", async () => {
    render(<Probe channels={["event://test/d"]} />);
    act(() => {
      emitTestEvent("event://test/d", { type: "a", payload: {} });
      emitTestEvent("event://test/d", { type: "b", payload: {} });
      emitTestEvent("event://test/d", { type: "c", payload: {} });
    });
    await screen.findByText("a,b,c", { selector: "[data-testid='types']" });
    expect(screen.getByTestId("count")).toHaveTextContent("3");
  });

  describe("session.delta sequencing (non-persistent stream ordinal)", () => {
    type Case = {
      name: string;
      emit: Array<{ seq?: number; text: string; afterFlush?: boolean }>;
      want: string[];
    };
    const cases: Case[] = [
      {
        name: "reorders out-of-order deltas by seq",
        emit: [
          { seq: 3, text: "t3" },
          { seq: 1, text: "t1" },
          { seq: 2, text: "t2" },
        ],
        want: ["t1", "t2", "t3"],
      },
      {
        name: "drops duplicate and replayed deltas idempotently",
        emit: [
          { seq: 1, text: "t1" },
          { seq: 2, text: "t2" },
          { seq: 2, text: "t2-again" },
          { seq: 1, text: "t1-replay" },
          { seq: 3, text: "t3" },
        ],
        want: ["t1", "t2", "t3"],
      },
      {
        name: "buffers a gap until the missing seq arrives",
        emit: [
          { seq: 1, text: "t1" },
          { seq: 3, text: "t3-held" },
          { seq: 2, text: "t2", afterFlush: true },
        ],
        want: ["t1", "t2", "t3-held"],
      },
      {
        name: "unsequenced legacy deltas pass through untouched",
        emit: [
          { text: "legacy-a" },
          { seq: 1, text: "t1" },
          { text: "legacy-b" },
        ],
        want: ["legacy-a", "t1", "legacy-b"],
      },
    ];

    for (const c of cases) {
      it(c.name, async () => {
        render(<DeltaProbe channels={[CH]} />);
        act(() => {
          for (const e of c.emit) {
            if (!e.afterFlush) emitTestEvent(CH, delta(e.seq, e.text));
          }
        });
        act(() => {
          for (const e of c.emit) {
            if (e.afterFlush) emitTestEvent(CH, delta(e.seq, e.text));
          }
        });
        await expectTexts(c.want);
      });
    }

    it("markDeltasApplied retires covered deltas incl. buffered ones", async () => {
      render(<DeltaProbe channels={[CH]} />);
      // t1 delivered; t3 buffered behind a gap.
      act(() => {
        emitTestEvent(CH, delta(1, "t1"));
        emitTestEvent(CH, delta(3, "t3-covered"));
      });
      await expectTexts(["t1"]);
      // A persisted message covering deltas ≤3 replaces the buffer.
      act(() => {
        fireEvent.click(screen.getByTestId("mark"));
      });
      // Covered replay is dropped…
      act(() => {
        emitTestEvent(CH, delta(3, "t3-replay"));
      });
      await expectTexts(["t1"]);
      // …and only newer deltas apply.
      act(() => {
        emitTestEvent(CH, delta(4, "t4"));
      });
      await expectTexts(["t1", "t4"]);
    });

    it("dropPendingDeltas freezes everything seen so far", async () => {
      render(<DeltaProbe channels={[CH]} />);
      act(() => {
        emitTestEvent(CH, delta(1, "t1"));
        emitTestEvent(CH, delta(3, "t3-straggler"));
      });
      await expectTexts(["t1"]);
      act(() => {
        fireEvent.click(screen.getByTestId("drop"));
      });
      act(() => {
        emitTestEvent(CH, delta(2, "t2-old")); // ≤ high-water: dropped
        emitTestEvent(CH, delta(4, "t4")); // newer: applies
      });
      await expectTexts(["t1", "t4"]);
    });

    it("resetDeltaTracking accepts a restarted counter (fresh kernel)", async () => {
      render(<DeltaProbe channels={[CH]} />);
      act(() => {
        emitTestEvent(CH, delta(1, "old-1"));
        emitTestEvent(CH, delta(2, "old-2"));
      });
      await expectTexts(["old-1", "old-2"]);
      act(() => {
        fireEvent.click(screen.getByTestId("reset"));
      });
      act(() => {
        emitTestEvent(CH, delta(1, "new-1"));
      });
      await expectTexts(["old-1", "old-2", "new-1"]);
    });
  });
});
