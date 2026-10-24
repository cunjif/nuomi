import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ReactNode } from "react";
import { emitTestEvent } from "../../lib/events/transport";
import { sessionChannel } from "../../lib/events/types";
import { seedConversation, tdState } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { ipc } from "../../lib/ipc/client";
import { useToastStore } from "../../lib/store/toastStore";
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

  it("keeps every message when multiple session.message events arrive in one batch", async () => {
    seedConversation({ id: "s7", title: "multi", createdAt: 1, updatedAt: 1 });
    useUiStore.getState().selectSession("s7");

    const qc = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={qc}>
        <ChatView />
      </QueryClientProvider>,
    );
    await screen.findByText(/暂无消息/);

    const ch = sessionChannel("s7");
    // Simulate a group-chat turn: user + two RoleAgent replies arrive as
    // persisted session.message events in a single batch. All three must
    // remain visible — the earlier ones must not be overwritten.
    act(() => {
      emitTestEvent(ch, {
        type: "session.message",
        sessionId: "s7",
        seq: 1,
        payload: { sessionId: "s7", role: "user", content: "你好呀", seq: 1 },
      });
      emitTestEvent(ch, {
        type: "session.message",
        sessionId: "s7",
        seq: 2,
        payload: { sessionId: "s7", role: "assistant", content: "回复一", seq: 2, deltaTo: 1 },
      });
      emitTestEvent(ch, {
        type: "session.message",
        sessionId: "s7",
        seq: 3,
        payload: { sessionId: "s7", role: "assistant", content: "回复二", seq: 3, deltaTo: 1 },
      });
    });

    await waitFor(() => expect(screen.getByText("你好呀")).toBeInTheDocument());
    expect(screen.getByText("回复一")).toBeInTheDocument();
    expect(screen.getByText("回复二")).toBeInTheDocument();
  });

  it("switches the whole transcript when the selected session changes", async () => {
    seedConversation({ id: "s9", title: "first", createdAt: 1, updatedAt: 1 });
    seedConversation({ id: "s10", title: "second", createdAt: 1, updatedAt: 1 });
    tdState.events.set("s9", [
      { seq: 1, kind: "message", payload: { role: "user", content: "第一个会话的问题" }, createdAt: 1 },
      { seq: 2, kind: "message", payload: { role: "assistant", content: "第一个会话的回答" }, createdAt: 2 },
    ]);
    tdState.events.set("s10", [
      { seq: 1, kind: "message", payload: { role: "user", content: "第二个会话的问题" }, createdAt: 1 },
    ]);
    useUiStore.getState().selectSession("s9");

    renderWithProviders(<ChatView />);
    expect(await screen.findByText("第一个会话的问题")).toBeInTheDocument();
    expect(screen.getByText("第一个会话的回答")).toBeInTheDocument();

    // Clicking a history row in the left rail only changes the selection.
    await act(async () => {
      useUiStore.getState().selectSession("s10");
    });

    await waitFor(() => expect(screen.getByText("第二个会话的问题")).toBeInTheDocument());
    expect(screen.queryByText("第一个会话的问题")).not.toBeInTheDocument();
    expect(screen.queryByText("第一个会话的回答")).not.toBeInTheDocument();
  });

  it("drops the previous session's live buffer when the selection changes", async () => {
    seedConversation({ id: "s11", title: "live-a", createdAt: 1, updatedAt: 1 });
    seedConversation({ id: "s12", title: "live-b", createdAt: 1, updatedAt: 1 });
    tdState.events.set("s12", [
      { seq: 1, kind: "message", payload: { role: "user", content: "另一个会话的问题" }, createdAt: 1 },
    ]);
    useUiStore.getState().selectSession("s11");

    renderWithProviders(<ChatView />);
    await screen.findByText(/暂无消息/);

    // Live traffic for the session we are about to leave. seq 7 is deliberately
    // outside the next session's history seqs so the history-overlap filter
    // cannot mask a leak.
    act(() => {
      emitTestEvent(sessionChannel("s11"), {
        type: "session.delta",
        sessionId: "s11",
        payload: { sessionId: "s11", text: "旧会话的流式回答" },
      });
      emitTestEvent(sessionChannel("s11"), {
        type: "session.message",
        sessionId: "s11",
        seq: 7,
        payload: { sessionId: "s11", role: "assistant", content: "旧会话的落库回答", seq: 7, deltaTo: 1 },
      });
    });
    await waitFor(() => expect(screen.getByText("旧会话的落库回答")).toBeInTheDocument());

    await act(async () => {
      useUiStore.getState().selectSession("s12");
    });

    await waitFor(() => expect(screen.getByText("另一个会话的问题")).toBeInTheDocument());
    expect(screen.queryByText("旧会话的流式回答")).not.toBeInTheDocument();
    expect(screen.queryByText("旧会话的落库回答")).not.toBeInTheDocument();
  });

  it("drops a live message once history refetch covers the same seq", async () => {
    seedConversation({ id: "s8", title: "dedupe", createdAt: 1, updatedAt: 1 });
    useUiStore.getState().selectSession("s8");

    const qc = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    render(
      <QueryClientProvider client={qc}>
        <ChatView />
      </QueryClientProvider>,
    );
    await screen.findByText(/暂无消息/);

    const ch = sessionChannel("s8");
    act(() => {
      emitTestEvent(ch, {
        type: "session.message",
        sessionId: "s8",
        seq: 1,
        payload: { sessionId: "s8", role: "assistant", content: "已持久化", seq: 1, deltaTo: 1 },
      });
    });
    await waitFor(() => expect(screen.getByText("已持久化")).toBeInTheDocument());

    // History refetch lands with the same seq — the live copy must be
    // dropped so the message renders exactly once.
    tdState.events.set("s8", [
      { seq: 1, kind: "message", payload: { role: "assistant", content: "已持久化" }, createdAt: 1 },
    ]);
    await act(async () => {
      await qc.invalidateQueries({ queryKey: ["sessionEvents", "s8"] });
    });
    await waitFor(() => expect(screen.getAllByText("已持久化")).toHaveLength(1));
  });
});

describe("ChatView — stop & failure notices (SPEC 5.8.1-5, 5.8.3)", () => {
  const INPUT_PLACEHOLDER = "输入消息，Enter 发送（Shift+Enter 换行）";
  type SubmitResult = Awaited<ReturnType<typeof ipc.submitTask>>;
  let resolveSubmit!: (value: SubmitResult) => void;
  let rejectSubmit!: (reason?: unknown) => void;

  function setup(session: string): void {
    seedConversation({ id: session, title: "t", createdAt: 1, updatedAt: 1 });
    useUiStore.getState().selectSession(session);
    vi.spyOn(ipc, "submitTask").mockImplementation(
      () =>
        new Promise<SubmitResult>((resolve, reject) => {
          resolveSubmit = resolve;
          rejectSubmit = reject;
        }) as unknown as ReturnType<typeof ipc.submitTask>,
    );
  }

  async function submitViaComposer(text: string): Promise<void> {
    const input = await screen.findByPlaceholderText(INPUT_PLACEHOLDER);
    fireEvent.change(input, { target: { value: text } });
    fireEvent.keyDown(input, { key: "Enter" });
  }

  afterEach(() => {
    vi.restoreAllMocks();
    useUiStore.setState({ selectedSessionId: null });
  });

  it("closes the pill, shows the stopped notice and inserts no late reply (5a)", async () => {
    setup("cs1");
    const stopSpy = vi.spyOn(ipc, "stopConversation").mockResolvedValue(null);
    renderWithProviders(<ChatView />);
    await screen.findByText(/暂无消息/);
    await submitViaComposer("帮忙看看");

    // The pill appears after the 300ms render delay.
    expect(await screen.findByTestId("status-pill")).toBeTruthy();

    fireEvent.click(within(screen.getByTestId("status-pill")).getByRole("button", { name: "停止" }));

    // Immediate close + stopped notice, stop dispatched to the backend.
    await waitFor(() => expect(screen.queryByTestId("status-pill")).toBeNull());
    expect(screen.getByTestId("turn-notice").getAttribute("data-kind")).toBe("stopped");
    expect(screen.getByText("⏹ 已停止")).toBeTruthy();
    expect(stopSpy).toHaveBeenCalledWith("cs1");

    // The cancelled turn settles with a (hypothetically late) reply —
    // nothing may be inserted and no empty-response toast may fire.
    await act(async () => {
      resolveSubmit({ finalText: "迟到的回复", steps: 1, truncated: false, sessionId: "cs1" });
    });
    await waitFor(() => expect(screen.queryByText("迟到的回复")).toBeNull());
    expect(useToastStore.getState().toasts).toHaveLength(0);
    expect(screen.getByTestId("turn-notice").getAttribute("data-kind")).toBe("stopped");
  });

  it("flags a failed turn with the error summary and a retry button (5.8.3-1c)", async () => {
    setup("cs2");
    renderWithProviders(<ChatView />);
    await screen.findByText(/暂无消息/);
    await submitViaComposer("提问");

    await act(async () => {
      rejectSubmit(new Error("provider unreachable"));
    });

    await waitFor(() =>
      expect(screen.getByTestId("turn-notice").getAttribute("data-kind")).toBe("failed"),
    );
    expect(screen.getByText(/Agent 执行失败/)).toBeTruthy();
    expect(screen.getByText(/provider unreachable/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "重试" })).toBeTruthy();
  });

  it("keeps received text and flags the interruption when the stream breaks (5.8.3-3c)", async () => {
    setup("cs3");
    renderWithProviders(<ChatView />);
    await screen.findByText(/暂无消息/);
    await submitViaComposer("提问");

    act(() => {
      emitTestEvent(sessionChannel("cs3"), {
        type: "session.delta",
        sessionId: "cs3",
        payload: { sessionId: "cs3", text: "已接收的部分" },
      });
    });
    await waitFor(() => expect(screen.getByText("已接收的部分")).toBeInTheDocument());

    await act(async () => {
      rejectSubmit(new Error("connection reset"));
    });

    await waitFor(() =>
      expect(screen.getByTestId("turn-notice").getAttribute("data-kind")).toBe("interrupted"),
    );
    expect(screen.getByText(/回复已中断/)).toBeTruthy();
    // Received content survives the interruption.
    expect(screen.getByText("已接收的部分")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "重试" })).toBeTruthy();
  });
});

describe("ChatView — speaker-column layout (胶囊不再被输入框覆盖)", () => {
  const INPUT_PLACEHOLDER = "输入消息，Enter 发送（Shift+Enter 换行）";
  type SubmitResult = Awaited<ReturnType<typeof ipc.submitTask>>;
  let resolveSubmit!: (value: SubmitResult) => void;

  function setup(session: string): void {
    seedConversation({ id: session, title: "t", createdAt: 1, updatedAt: 1 });
    useUiStore.getState().selectSession(session);
    vi.spyOn(ipc, "submitTask").mockImplementation(
      () =>
        new Promise<SubmitResult>((resolve) => {
          resolveSubmit = resolve;
        }) as unknown as ReturnType<typeof ipc.submitTask>,
    );
  }

  async function submitViaComposer(text: string): Promise<void> {
    const input = await screen.findByPlaceholderText(INPUT_PLACEHOLDER);
    fireEvent.change(input, { target: { value: text } });
    fireEvent.keyDown(input, { key: "Enter" });
  }

  afterEach(() => {
    vi.restoreAllMocks();
    useUiStore.setState({ selectedSessionId: null });
  });

  it("renders the status pill and agent turn row INSIDE the scroll container", async () => {
    setup("cl1");
    renderWithProviders(<ChatView />);
    await screen.findByText(/暂无消息/);
    await submitViaComposer("你好");

    expect(await screen.findByTestId("agent-turn-row")).toBeTruthy();

    // The overlap regression guard: the pill must live inside the message
    // scroll container (in-flow), never as a sibling of it — a sibling of
    // the h-full scroller is what got rendered under the composer.
    const pill = await screen.findByTestId("status-pill");
    const scroll = screen.getByTestId("message-scroll");
    expect(scroll.contains(pill)).toBe(true);

    // The agent turn row also sits in the scroll flow, after the user row.
    expect(scroll.contains(screen.getByTestId("agent-turn-row"))).toBe(true);

    await act(async () => {
      resolveSubmit({ finalText: "好", steps: 1, truncated: false, sessionId: "cl1" });
    });
  });

  it("renders the user speaker column (你) and agent name on messages", async () => {
    seedConversation({ id: "cl2", title: "t", createdAt: 1, updatedAt: 1 });
    tdState.events.set("cl2", [
      { seq: 1, kind: "message", payload: { role: "user", content: "第一条" }, createdAt: 1 },
      { seq: 2, kind: "message", payload: { role: "assistant", role_name: "后端专家", content: "回复" }, createdAt: 2 },
    ]);
    useUiStore.getState().selectSession("cl2");

    renderWithProviders(<ChatView />);
    expect(await screen.findByText("第一条")).toBeInTheDocument();

    // User row carries the fixed 你 identity column; the assistant row
    // carries the speaking agent's name.
    expect(screen.getAllByText("你").length).toBeGreaterThan(0);
    expect(screen.getByText("后端专家")).toBeInTheDocument();
  });

  it("returns the pill when the CLI stream stalls after its first line (idle watchdog)", async () => {
    setup("cl3");
    renderWithProviders(<ChatView />);
    await screen.findByText(/暂无消息/);
    await submitViaComposer("跑一下构建");

    // CLI agent's first stdout line arrives immediately — inside the
    // 300ms render window, killing the initial pill appearance.
    act(() => {
      emitTestEvent(sessionChannel("cl3"), {
        type: "session.delta",
        sessionId: "cl3",
        payload: { sessionId: "cl3", text: "ok" },
      });
    });
    await waitFor(() => expect(screen.getByText("ok")).toBeInTheDocument());
    await waitFor(() => expect(screen.queryByTestId("status-pill")).toBeNull(), { timeout: 1500 });

    // Then the CLI agent falls silent (framework boot / tool run). The
    // idle watchdog must bring the pill back with the still-working copy.
    await waitFor(
      () => expect(screen.getByTestId("status-pill")).toBeTruthy(),
      { timeout: 6000 },
    );
    expect(screen.getByText(/仍在处理中/)).toBeTruthy();

    await act(async () => {
      resolveSubmit({ finalText: "done", steps: 1, truncated: false, sessionId: "cl3" });
    });
  });
});

describe("ChatView — WeChat-style bubble toolbar (气泡工具栏)", () => {
  const USER_TEXT = "帮我看看这个 bug";
  const AI_TEXT = "这是分析结果";

  /** Walks up from a text node to the bubble body (the max-w-[80%] div). */
  function bubbleOf(text: string): HTMLElement {
    const el = screen.getByText(text);
    const bubble = el.closest("div.max-w-\\[80\\%\\]");
    if (bubble === null) throw new Error(`bubble for "${text}" not found`);
    return bubble as HTMLElement;
  }

  async function setupHistory(session: string): Promise<void> {
    seedConversation({ id: session, title: "t", createdAt: 1, updatedAt: 1 });
    tdState.events.set(session, [
      { seq: 1, kind: "message", payload: { role: "user", content: USER_TEXT }, createdAt: 1 },
      { seq: 2, kind: "message", payload: { role: "assistant", content: AI_TEXT }, createdAt: 2 },
    ]);
    useUiStore.getState().selectSession(session);
    renderWithProviders(<ChatView />);
    await screen.findByText(USER_TEXT);
    await screen.findByText(AI_TEXT);
  }

  afterEach(() => {
    vi.restoreAllMocks();
    delete (navigator as { clipboard?: unknown }).clipboard;
    useUiStore.setState({ selectedSessionId: null });
  });

  it("draws a hairline border on the assistant bubble but not the user bubble", async () => {
    await setupHistory("bt1");
    expect(bubbleOf(AI_TEXT).className).toContain("border-ink-muted/40");
    expect(bubbleOf(USER_TEXT).className).not.toContain("border-ink-muted/40");
  });

  it("pops the toolbar above the assistant bubble on click (quote + copy, no rollback)", async () => {
    await setupHistory("bt2");
    fireEvent.click(bubbleOf(AI_TEXT));

    const toolbar = screen.getByTestId("bubble-toolbar");
    expect(within(toolbar).getByRole("button", { name: "引用" })).toBeTruthy();
    expect(within(toolbar).getByRole("button", { name: "复制" })).toBeTruthy();
    expect(within(toolbar).queryByRole("button", { name: /回退/ })).toBeNull();
  });

  it("keeps the toolbar on a single line even over a narrow bubble", async () => {
    // Regression guard: the toolbar used to shrink to the containing
    // bubble's width (width:auto shrink-to-fit), wrapping 回退到此处
    // vertically. It must size to its content and never wrap.
    seedConversation({ id: "bt7", title: "t", createdAt: 1, updatedAt: 1 });
    tdState.events.set("bt7", [
      { seq: 1, kind: "message", payload: { role: "user", content: "短" }, createdAt: 1 },
    ]);
    useUiStore.getState().selectSession("bt7");
    renderWithProviders(<ChatView />);
    await screen.findByText("短");

    fireEvent.click(bubbleOf("短"));
    const toolbar = screen.getByTestId("bubble-toolbar");
    expect(toolbar.className).toContain("w-max");
    expect(toolbar.className).toContain("whitespace-nowrap");
    const rollbackBtn = within(toolbar).getByRole("button", { name: "回退到此处" });
    expect(rollbackBtn.className).toContain("whitespace-nowrap");
    expect(rollbackBtn.querySelector("span")?.textContent).toBe("回退到此处");
  });

  it("opens a gap between user and assistant message rows (WeChat grouping)", async () => {
    // Role switches (user ↔ agent) breathe; the first row has no gap.
    await setupHistory("bt8");
    const userRow = bubbleOf(USER_TEXT).parentElement as HTMLElement;
    const aiRow = bubbleOf(AI_TEXT).parentElement as HTMLElement;
    expect(userRow.className).not.toContain("mt-2");
    expect(aiRow.className).toContain("mt-2");
  });

  it("stages a quote preview after picking 引用 from the toolbar", async () => {
    await setupHistory("bt3");
    fireEvent.click(bubbleOf(AI_TEXT));
    fireEvent.click(within(screen.getByTestId("bubble-toolbar")).getByRole("button", { name: "引用" }));

    // Toolbar closes; the composer quote preview bar carries the excerpt.
    expect(screen.queryByTestId("bubble-toolbar")).toBeNull();
    expect(await screen.findByText("引用")).toBeTruthy();
    // The excerpt shows in the preview bar next to the original bubble.
    expect(screen.getAllByText(AI_TEXT).length).toBeGreaterThanOrEqual(2);

    // Submit carries the quoted seq.
    const submitSpy = vi.spyOn(ipc, "submitTask").mockResolvedValue({
      finalText: "", steps: 0, truncated: false, sessionId: "bt3",
    } as unknown as Awaited<ReturnType<typeof ipc.submitTask>>);
    const input = screen.getByPlaceholderText("输入消息，Enter 发送（Shift+Enter 换行）");
    fireEvent.change(input, { target: { value: "追问" } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => expect(submitSpy).toHaveBeenCalledWith("bt3", "追问", 2));
  });

  it("offers 回退到此处 only on user bubbles and opens the confirm dialog", async () => {
    await setupHistory("bt4");
    fireEvent.click(bubbleOf(USER_TEXT));

    const toolbar = screen.getByTestId("bubble-toolbar");
    fireEvent.click(within(toolbar).getByRole("button", { name: "回退到此处" }));

    expect(screen.queryByTestId("bubble-toolbar")).toBeNull();
    expect(await screen.findByText("确认回退")).toBeTruthy();
  });

  it("closes the toolbar on a click outside any message row", async () => {
    await setupHistory("bt5");
    fireEvent.click(bubbleOf(AI_TEXT));
    expect(screen.getByTestId("bubble-toolbar")).toBeTruthy();

    fireEvent.mouseDown(document.body);
    expect(screen.queryByTestId("bubble-toolbar")).toBeNull();
  });

  it("copies the message text via the clipboard API", async () => {
    await setupHistory("bt6");
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });

    fireEvent.click(bubbleOf(AI_TEXT));
    fireEvent.click(within(screen.getByTestId("bubble-toolbar")).getByRole("button", { name: "复制" }));

    await waitFor(() => expect(writeText).toHaveBeenCalledWith(AI_TEXT));
    await waitFor(() => expect(useToastStore.getState().toasts.some((t) => t.message === "已复制")).toBe(true));
  });
});
