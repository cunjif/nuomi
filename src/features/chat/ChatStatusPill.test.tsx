import { act, render, screen } from "@testing-library/react";
import type { ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderWithProviders } from "../../test/helpers";
import { scanRawPaletteClasses } from "../../test/theme-tokens";
import { ChatStatusPill } from "./ChatStatusPill";
import type { ChatStatusPillProps } from "./ChatStatusPill";
import type { ChatEntry } from "./useSessionStream";

const START = 1_000_000;
const INITIAL = "🤔 正在思考…";

function toolEntry(id: string, tool: string, args?: Record<string, unknown>): ChatEntry {
  return { id, kind: "tool_call", tool, argsJson: JSON.stringify(args ?? {}) };
}

interface Harness {
  rerender: (props: ChatStatusPillProps) => void;
}

/** The root element type must stay stable across rerenders, otherwise React
 *  remounts the whole tree and resets the pill's turn state. */
function renderPill(initial: ChatStatusPillProps): Harness {
  const qc = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  });
  const view = (props: ChatStatusPillProps): ReactElement => (
    <QueryClientProvider client={qc}>
      <ChatStatusPill {...props} />
    </QueryClientProvider>
  );
  const result = render(view(initial));
  return { rerender: (props) => result.rerender(view(props)) };
}

describe("ChatStatusPill — single-chat signal wiring", () => {
  beforeEach(() => {
    vi.useFakeTimers({ now: START });
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("appears after the 300ms delay with the initial status and timer", () => {
    renderWithProviders(
      <ChatStatusPill isPending={true} hasStreamText={false} hasError={false} entries={[]} />,
    );
    expect(screen.queryByTestId("status-pill")).toBeNull();
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(screen.getByTestId("status-pill")).toBeTruthy();
    expect(screen.getByText(INITIAL)).toBeTruthy();
    expect(screen.getByText("0s")).toBeTruthy();
  });

  it("updates to the tool-call status when a tool.call arrives mid-turn", () => {
    const history: ChatEntry[] = [toolEntry("c1", "read_file", { path: "old.rs" })];
    const h = renderPill({ isPending: true, hasStreamText: false, hasError: false, entries: history });
    act(() => {
      vi.advanceTimersByTime(300);
    });
    // Pre-turn history tool calls are NOT replayed as signals.
    expect(screen.getByText(INITIAL)).toBeTruthy();

    h.rerender({
      isPending: true,
      hasStreamText: false,
      hasError: false,
      entries: [...history, toolEntry("live-c0", "write_file", { path: "src/lib.rs" })],
    });
    expect(screen.getByText("🔧 正在调用工具 write_file（src/lib.rs）…")).toBeTruthy();
  });

  it("uses the tool-name-only status when args carry no string value", () => {
    const h = renderPill({ isPending: true, hasStreamText: false, hasError: false, entries: [] });
    act(() => {
      vi.advanceTimersByTime(300);
    });
    h.rerender({
      isPending: true,
      hasStreamText: false,
      hasError: false,
      entries: [toolEntry("live-c0", "list_dir", { limit: 10 })],
    });
    expect(screen.getByText("🔧 正在调用工具 list_dir…")).toBeTruthy();
  });

  it("degrades to rotating fun copy after 10s without any signal", () => {
    renderWithProviders(
      <ChatStatusPill isPending={true} hasStreamText={false} hasError={false} entries={[]} />,
    );
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(screen.getByText(INITIAL)).toBeTruthy();

    act(() => {
      vi.advanceTimersByTime(10_000);
    });
    const statusText = screen.getByRole("status").textContent;
    expect(statusText).not.toBe(INITIAL);
    expect(statusText).not.toBeNull();
  });

  it("fades out on the first stream token and unmounts after the transition", () => {
    const h = renderPill({ isPending: true, hasStreamText: false, hasError: false, entries: [] });
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(screen.getByTestId("status-pill")).toBeTruthy();

    h.rerender({ isPending: true, hasStreamText: true, hasError: false, entries: [] });
    expect(screen.getByTestId("status-pill").className).toContain("opacity-0");

    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(screen.queryByTestId("status-pill")).toBeNull();
  });

  it("never flashes a pill when the first token beats the 300ms delay", () => {
    const h = renderPill({ isPending: true, hasStreamText: false, hasError: false, entries: [] });
    h.rerender({ isPending: true, hasStreamText: true, hasError: false, entries: [] });
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.queryByTestId("status-pill")).toBeNull();
  });

  it("does not reappear in the same turn once the reply started (latch)", () => {
    const h = renderPill({ isPending: true, hasStreamText: false, hasError: false, entries: [] });
    act(() => {
      vi.advanceTimersByTime(300);
    });
    h.rerender({ isPending: true, hasStreamText: true, hasError: false, entries: [] });
    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(screen.queryByTestId("status-pill")).toBeNull();

    // Durable message landed: stream text cleared, but the turn is still
    // pending — the pill must stay gone (spec 5.8.1-6a).
    h.rerender({ isPending: true, hasStreamText: false, hasError: false, entries: [] });
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(screen.queryByTestId("status-pill")).toBeNull();

    // The next turn (pending restarts) gets a fresh pill.
    h.rerender({ isPending: false, hasStreamText: false, hasError: false, entries: [] });
    h.rerender({ isPending: true, hasStreamText: false, hasError: false, entries: [] });
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(screen.getByTestId("status-pill")).toBeTruthy();
  });

  it("hides immediately on error and tears down cleanly on turn end", () => {
    const h = renderPill({ isPending: true, hasStreamText: false, hasError: false, entries: [] });
    act(() => {
      vi.advanceTimersByTime(300);
    });
    h.rerender({ isPending: true, hasStreamText: false, hasError: true, entries: [] });
    expect(screen.queryByTestId("status-pill")).toBeNull();

    h.rerender({ isPending: false, hasStreamText: false, hasError: false, entries: [] });
    expect(screen.queryByTestId("status-pill")).toBeNull();
  });

  it("uses only semantic theme tokens (no raw palette classes)", () => {
    const { container } = renderWithProviders(
      <ChatStatusPill
        isPending={true}
        hasStreamText={false}
        hasError={false}
        entries={[toolEntry("live-t0", "write_file", { path: "src/lib.rs" })]}
        onStop={() => {}}
      />,
    );
    act(() => {
      vi.advanceTimersByTime(300);
    });
    expect(screen.getByTestId("status-pill")).toBeTruthy();
    expect(scanRawPaletteClasses(container)).toEqual([]);
  });
});
