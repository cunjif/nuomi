import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { useState, type ReactNode } from "react";
import { emitTestEvent, resetTestBus } from "./transport";
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
});
