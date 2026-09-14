import { act, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { emitTestEvent } from "../../lib/events/transport";
import { sessionChannel } from "../../lib/events/types";
import { seedConversation, tdState } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import "../../i18n";
import { ChatView } from "./ChatView";
import { useUiStore } from "../../lib/store/uiStore";

describe("ChatView — live token stream (AC10)", () => {
  it("appends streamed deltas as an assistant bubble", async () => {
    seedConversation({ id: "s1", title: "demo", createdAt: 1, updatedAt: 1 });
    useUiStore.getState().selectSession("s1");

    renderWithProviders(<ChatView />);
    await screen.findByText(/暂无消息/);

    act(() => {
      emitTestEvent(sessionChannel("s1"), {
        type: "session.delta",
        sessionId: "s1",
        payload: { sessionId: "s1", text: "你好，" },
      });
    });
    act(() => {
      emitTestEvent(sessionChannel("s1"), {
        type: "session.delta",
        sessionId: "s1",
        payload: { sessionId: "s1", text: "世界" },
      });
    });

    // rAF batching may merge both deltas into one bubble.
    await waitFor(() => expect(screen.getByText(/你好，/)).toBeInTheDocument());
    await waitFor(() => expect(screen.getByText(/世界/)).toBeInTheDocument());
  });

  it("renders persisted history events", async () => {
    seedConversation({ id: "s2", title: "hist", createdAt: 1, updatedAt: 1 });
    tdState.events.set("s2", [
      { seq: 1, kind: "message", payload: { role: "user", content: "帮我看看这个 bug" }, createdAt: 1 },
      {
        seq: 2,
        kind: "tool_call",
        payload: { tool: "read_file", arguments: { path: "src/main.rs" } },
        createdAt: 2,
      },
    ]);
    useUiStore.getState().selectSession("s2");

    renderWithProviders(<ChatView />);
    expect(await screen.findByText("帮我看看这个 bug")).toBeInTheDocument();
    expect(screen.getByText("read_file")).toBeInTheDocument();
  });

  it("swaps the live buffer for the persisted message without duplication", async () => {
    seedConversation({ id: "s5", title: "swap", createdAt: 1, updatedAt: 1 });
    useUiStore.getState().selectSession("s5");

    const qc = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={qc}>
        <ChatView />
      </QueryClientProvider>,
    );
    await screen.findByText(/暂无消息/);

    const ch = sessionChannel("s5");
    act(() => {
      emitTestEvent(ch, {
        type: "session.delta",
        sessionId: "s5",
        seq: 1,
        payload: { sessionId: "s5", seq: 1, text: "你好" },
      });
      // Replayed delta with an already-applied ordinal must be ignored.
      emitTestEvent(ch, {
        type: "session.delta",
        sessionId: "s5",
        seq: 1,
        payload: { sessionId: "s5", seq: 1, text: "你好" },
      });
      emitTestEvent(ch, {
        type: "session.delta",
        sessionId: "s5",
        seq: 2,
        payload: { sessionId: "s5", seq: 2, text: "呀" },
      });
    });
    await waitFor(() => expect(screen.getByText("你好呀")).toBeInTheDocument());

    // Persisted authority arrives: buffer is replaced by the full text.
    act(() => {
      emitTestEvent(ch, {
        type: "session.message",
        sessionId: "s5",
        seq: 1,
        payload: { sessionId: "s5", role: "assistant", content: "你好呀，世界", seq: 1, deltaTo: 2 },
      });
    });
    await waitFor(() => expect(screen.getByText("你好呀，世界")).toBeInTheDocument());
    expect(screen.queryByText("你好呀")).not.toBeInTheDocument();

    // A late straggler inside the covered range is not appended again.
    act(() => {
      emitTestEvent(ch, {
        type: "session.delta",
        sessionId: "s5",
        seq: 2,
        payload: { sessionId: "s5", seq: 2, text: "呀" },
      });
    });
    await waitFor(() =>
      expect(screen.queryByText("你好呀，世界呀")).not.toBeInTheDocument(),
    );
  });

  it("backfilled history after refetch renders each message exactly once", async () => {
    seedConversation({ id: "s6", title: "backfill", createdAt: 1, updatedAt: 1 });
    tdState.events.set("s6", [
      { seq: 1, kind: "message", payload: { role: "user", content: "第一条" }, createdAt: 1 },
    ]);
    useUiStore.getState().selectSession("s6");

    const qc = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    function Wrapped(): ReactNode {
      return (
        <QueryClientProvider client={qc}>
          <ChatView />
        </QueryClientProvider>
      );
    }
    render(<Wrapped />);
    expect(await screen.findByText("第一条")).toBeInTheDocument();

    // Simulate reconnect backfill: the refetched log contains the same full
    // history (plus a newer row); nothing already rendered may duplicate.
    tdState.events.set("s6", [
      { seq: 1, kind: "message", payload: { role: "user", content: "第一条" }, createdAt: 1 },
      { seq: 2, kind: "message", payload: { role: "assistant", content: "回复" }, createdAt: 2 },
    ]);
    await act(async () => {
      await qc.invalidateQueries({ queryKey: ["sessionEvents", "s6"] });
    });
    await waitFor(() => expect(screen.getAllByText("第一条")).toHaveLength(1));
    expect(screen.getAllByText("回复")).toHaveLength(1);
  });
});
