import { act, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { emitTestEvent } from "../../lib/events/transport";
import { sessionChannel } from "../../lib/events/types";
import { tdState } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { ChatView } from "./ChatView";
import { useUiStore } from "../../lib/store/uiStore";

describe("ChatView — live token stream (AC10)", () => {
  it("appends streamed deltas as an assistant bubble", async () => {
    tdState.sessions.push({ id: "s1", title: "demo", createdAt: 1, updatedAt: 1 });
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
    tdState.sessions.push({ id: "s2", title: "hist", createdAt: 1, updatedAt: 1 });
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
});
