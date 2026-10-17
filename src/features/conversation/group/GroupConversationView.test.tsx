import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderWithProviders } from "../../../test/helpers";
import { emitTestEvent } from "../../../lib/events/transport";
import { sessionChannel, type DomainEvent } from "../../../lib/events/types";
import { ipc } from "../../../lib/ipc/client";
import { tdState } from "../../../lib/ipc/test-double-state";
import { useUiStore } from "../../../lib/store/uiStore";
import { GroupConversationView } from "./GroupConversationView";

const SESSION = "grp-1";
const CHANNEL = sessionChannel(SESSION);
const INPUT_PLACEHOLDER = "输入消息，Enter 发送（Shift+Enter 换行）";

type SubmitResult = Awaited<ReturnType<typeof ipc.submitMessage>>;
let resolveSubmit!: (value: SubmitResult) => void;
let rejectSubmit!: (reason?: unknown) => void;

function emit(type: string, payload: Record<string, unknown>, seq?: number): void {
  const event: DomainEvent = {
    type,
    sessionId: SESSION,
    seq,
    payload: { sessionId: SESSION, ...payload },
  };
  emitTestEvent(CHANNEL, event);
}

async function submitViaComposer(text: string): Promise<void> {
  const input = await screen.findByPlaceholderText(INPUT_PLACEHOLDER);
  fireEvent.change(input, { target: { value: text } });
  fireEvent.keyDown(input, { key: "Enter" });
}

beforeEach(() => {
  tdState.events.set(SESSION, []);
  useUiStore.setState({ selectedSessionId: SESSION });
  vi.spyOn(ipc, "submitMessage").mockImplementation(
    () =>
      new Promise<SubmitResult>((resolve, reject) => {
        resolveSubmit = resolve;
        rejectSubmit = reject;
      }) as unknown as ReturnType<typeof ipc.submitMessage>,
  );
});

afterEach(() => {
  vi.restoreAllMocks();
  useUiStore.setState({ selectedSessionId: null });
});

describe("GroupConversationView — scrolling group pill (SPEC 5.8.1-4)", () => {
  it("shows the pill after submit and scrolls status lines as work proceeds (4a)", async () => {
    renderWithProviders(<GroupConversationView />);
    await submitViaComposer("讨论一下");

    expect(screen.getByTestId("group-pill-slot")).toBeTruthy();
    expect(screen.getByText("🫖 正在等待 Agent 响应…")).toBeTruthy();

    act(() => {
      emit(
        "session.selector_decision",
        { strategy: "round_robin", chosenRoleId: "r1", chosenRoleName: "Writer", converged: false },
        1,
      );
    });
    await waitFor(() => expect(screen.getByText("💭 Writer 正在思考…")).toBeInTheDocument());

    // Expand the history: the waiting line is kept as a status entry.
    fireEvent.click(screen.getByRole("button", { name: "展开状态详情" }));
    await waitFor(() =>
      expect(screen.getAllByText("🫖 正在等待 Agent 响应…").length).toBeGreaterThanOrEqual(1),
    );
  });

  it("fades the pill into the agent's streaming bubble on the first delta (4b)", async () => {
    renderWithProviders(<GroupConversationView />);
    await submitViaComposer("讨论");
    act(() => {
      emit(
        "session.selector_decision",
        { chosenRoleId: "r1", chosenRoleName: "Writer", converged: false },
        1,
      );
    });
    await waitFor(() => expect(screen.getByText("💭 Writer 正在思考…")).toBeInTheDocument());

    act(() => {
      emit("session.delta", { text: "你好，", roleId: "r1", roleName: "Writer", seq: 1 });
      emit("session.delta", { text: "大家好", roleId: "r1", roleName: "Writer", seq: 2 });
    });

    // Streaming bubble: speaker tag + accumulated text.
    await waitFor(() => expect(screen.getByText("你好，大家好")).toBeInTheDocument());
    expect(screen.getByText("Writer")).toBeTruthy();
    // The pill is fading out (closing) while the bubble takes over.
    await waitFor(() =>
      expect(screen.getByTestId("status-pill").className).toContain("opacity-0"),
    );
    await waitFor(() => expect(screen.queryByTestId("status-pill")).toBeNull(), { timeout: 1500 });
  });

  it("re-seeds the waiting pill between speakers and swaps avatars (4c)", async () => {
    renderWithProviders(<GroupConversationView />);
    await submitViaComposer("讨论");

    // Turn 1 — Writer speaks.
    act(() => {
      emit(
        "session.selector_decision",
        { chosenRoleId: "r1", chosenRoleName: "Writer", converged: false },
        1,
      );
      emit("session.delta", { text: "我先说", roleId: "r1", roleName: "Writer", seq: 1 });
    });
    // Durable record lands → utterance completes.
    act(() => {
      emit(
        "session.message",
        { role_name: "Writer", roleId: "r1", roleName: "Writer", content: "我先说", seq: 2 },
        2,
      );
    });
    // Between turns the pill returns as the next-agent waiting state.
    await waitFor(() =>
      expect(screen.getByText("⏭️ 等待下一位 Agent 发言…")).toBeInTheDocument(),
    );

    // Turn 2 — Reviewer takes the floor.
    act(() => {
      emit(
        "session.selector_decision",
        { chosenRoleId: "r2", chosenRoleName: "Reviewer", converged: false },
        3,
      );
      emit("session.delta", { text: "我补充", roleId: "r2", roleName: "Reviewer", seq: 1 });
    });
    await waitFor(() => expect(screen.getByText("我补充")).toBeInTheDocument());
    expect(screen.getByText("Reviewer")).toBeTruthy();
    // Both agents' bubbles coexist with their own avatars.
    expect(screen.getByText("我先说")).toBeTruthy();
  });

  it("clears the pill when the run settles", async () => {
    renderWithProviders(<GroupConversationView />);
    await submitViaComposer("讨论");
    expect(screen.getByTestId("group-pill-slot")).toBeTruthy();

    await act(async () => {
      resolveSubmit({ finalText: "", steps: 0, truncated: false, sessionId: SESSION });
    });
    await waitFor(() => expect(screen.queryByTestId("group-pill-slot")).toBeNull());
  });

  it("renders durable group history with per-agent names from the event log", async () => {
    tdState.events.set(SESSION, [
      { seq: 1, kind: "message", payload: { role_name: "Writer", content: "历史发言" }, createdAt: 1 },
    ]);
    renderWithProviders(<GroupConversationView />);
    await waitFor(() => expect(screen.getByText("历史发言")).toBeInTheDocument());
    expect(screen.getByText("Writer")).toBeTruthy();
  });
});

describe("GroupConversationView — stop & failure notices (SPEC 5.8.1-5, 5.8.3)", () => {
  it("closes the pill immediately and shows the stopped notice (5a)", async () => {
    const stopSpy = vi.spyOn(ipc, "stopConversation").mockResolvedValue(null);
    renderWithProviders(<GroupConversationView />);
    await submitViaComposer("讨论");
    expect(screen.getByTestId("group-pill-slot")).toBeTruthy();

    fireEvent.click(within(screen.getByTestId("status-pill")).getByRole("button", { name: "停止" }));

    // Immediate: the pill closes without waiting for the backend.
    await waitFor(() => expect(screen.queryByTestId("group-pill-slot")).toBeNull());
    expect(screen.getByTestId("turn-notice").getAttribute("data-kind")).toBe("stopped");
    expect(screen.getByText("⏹ 已停止")).toBeTruthy();
    expect(stopSpy).toHaveBeenCalledWith(SESSION);

    // The cancelled turn settles — the stopped notice stays; no failure or
    // interrupted notice replaces it and no further content is inserted.
    await act(async () => {
      resolveSubmit({ finalText: "", steps: 0, truncated: false, sessionId: SESSION });
    });
    expect(screen.getByTestId("turn-notice").getAttribute("data-kind")).toBe("stopped");
    expect(screen.queryByText(/Agent 执行失败/)).toBeNull();
    expect(screen.queryByText(/回复已中断/)).toBeNull();
  });

  it("shows the failure summary with a retry affordance (5.8.3-1c)", async () => {
    renderWithProviders(<GroupConversationView />);
    await submitViaComposer("讨论");
    await act(async () => {
      rejectSubmit(new Error("provider down"));
    });

    await waitFor(() =>
      expect(screen.getByTestId("turn-notice").getAttribute("data-kind")).toBe("failed"),
    );
    expect(screen.getByText(/Agent 执行失败/)).toBeTruthy();
    expect(screen.getByText(/provider down/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "重试" })).toBeTruthy();
  });

  it("keeps received content and flags the interruption (5.8.3-3c)", async () => {
    renderWithProviders(<GroupConversationView />);
    await submitViaComposer("讨论");
    act(() => {
      emit("session.delta", { text: "部分回答", roleId: "r1", roleName: "Writer", seq: 1 });
    });
    await waitFor(() => expect(screen.getByText("部分回答")).toBeInTheDocument());

    await act(async () => {
      rejectSubmit(new Error("connection reset"));
    });

    await waitFor(() =>
      expect(screen.getByTestId("turn-notice").getAttribute("data-kind")).toBe("interrupted"),
    );
    expect(screen.getByText(/回复已中断/)).toBeTruthy();
    // Received content survives the interruption.
    expect(screen.getByText("部分回答")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重试" })).toBeTruthy();
  });
});
