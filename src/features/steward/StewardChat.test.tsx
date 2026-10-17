import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { testDoubleCommands } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import type {
  IpcError,
  Result,
  StewardReplyDto,
  StewardSessionDto,
} from "../../lib/ipc/bindings.gen";
import { emitTestEvent } from "../../lib/events/transport";
import { sessionChannel } from "../../lib/events/types";
import { StewardChat } from "./StewardChat";

const okResult = <T,>(data: T): Result<T, IpcError> => ({ status: "ok", data });
const errResult = (code: string, message: string): Result<never, IpcError> => ({
  status: "error",
  error: { generic: { code, message } },
});
const session = (id: string): StewardSessionDto => ({
  id,
  stewardId: "steward",
  title: "管家会话",
  goal: null,
  createdAt: 1,
  updatedAt: 1,
});

async function setupStewardSession(overrides?: {
  send?: (
    sessionId: string,
    text: string,
    attachmentIds: string[] | null,
  ) => Promise<Result<StewardReplyDto, IpcError>>;
}): Promise<string> {
  const base = testDoubleCommands();
  injectIpcCommands({
    ...base,
    stewardEnsureSingletonSession: vi.fn(async () => okResult(session("st-s1"))),
    stewardSendMessage:
      overrides?.send ??
      vi.fn(async () => okResult<StewardReplyDto>({ type: "text", content: "test double" })),
  });
  renderWithProviders(<StewardChat />);
  await screen.findByPlaceholderText("向管家发送消息...");
  return "st-s1";
}

async function submitDraft(text: string): Promise<void> {
  const input = screen.getByPlaceholderText("向管家发送消息...");
  fireEvent.change(input, { target: { value: text } });
  fireEvent.click(screen.getByRole("button", { name: "发送" }));
}

describe("StewardChat — singleton window", () => {
  it("renders without session list rail and no '新建会话' button", async () => {
    const base = testDoubleCommands();
    const ensure = vi.fn(base.stewardEnsureSingletonSession);
    injectIpcCommands({ ...base, stewardEnsureSingletonSession: ensure });

    renderWithProviders(<StewardChat />);

    await waitFor(() => expect(ensure).toHaveBeenCalled());

    expect(screen.queryByText("新建会话")).toBeNull();
    expect(screen.queryByText("选择或创建一个管家会话")).toBeNull();
    expect(screen.queryByText("+ 新建会话")).toBeNull();
  });

  it("calls stewardEnsureSingletonSession on mount and shows ready prompt", async () => {
    const base = testDoubleCommands();
    const ensure = vi.fn(base.stewardEnsureSingletonSession);
    injectIpcCommands({ ...base, stewardEnsureSingletonSession: ensure });

    renderWithProviders(<StewardChat />);

    await waitFor(() => expect(ensure).toHaveBeenCalledTimes(1));
    await screen.findByText(/向管家发送消息/);
  });

  it("shows degraded prompt when ensure fails", async () => {
    const base = testDoubleCommands();
    const ensure = vi.fn(async () => {
      throw new Error("store_unavailable");
    });
    injectIpcCommands({ ...base, stewardEnsureSingletonSession: ensure });

    renderWithProviders(<StewardChat />);

    await screen.findByText(/会话历史暂时不可用/);
  });

  it("always renders input area even before session is ready", async () => {
    const base = testDoubleCommands();
    injectIpcCommands(base);

    renderWithProviders(<StewardChat />);

    await screen.findByPlaceholderText("向管家发送消息...");
  });
});

describe("StewardChat v2 — streaming render", () => {
  it("streams deltas then takes over with the authoritative steward.message", async () => {
    let resolveSend: (r: Result<StewardReplyDto, IpcError>) => void = () => {};
    const send = vi.fn(
      () =>
        new Promise<Result<StewardReplyDto, IpcError>>((res) => {
          resolveSend = res;
        }),
    );
    const sid = await setupStewardSession({ send });

    await submitDraft("你好");
    await waitFor(() => expect(send).toHaveBeenCalledWith("st-s1", "你好", null));

    const ch = sessionChannel(sid);
    act(() => {
      emitTestEvent(ch, {
        type: "steward.delta",
        sessionId: sid,
        payload: { sessionId: sid, text: "你好，", seq: 1 },
      });
    });
    act(() => {
      emitTestEvent(ch, {
        type: "steward.delta",
        sessionId: sid,
        payload: { sessionId: sid, text: "我是管家。", seq: 2 },
      });
    });
    // rAF batching may merge deltas — the accumulated buffer must render.
    await waitFor(() => expect(screen.getByText("你好，我是管家。")).toBeInTheDocument());
    expect(screen.getByText("▍")).toBeInTheDocument();

    // Authoritative takeover: steward.message replaces the streamed buffer.
    act(() => {
      emitTestEvent(ch, {
        type: "steward.message",
        sessionId: sid,
        payload: { sessionId: sid, text: "你好，我是糯米管家，请问有什么可以帮你？" },
      });
    });
    await waitFor(() =>
      expect(screen.getByText("你好，我是糯米管家，请问有什么可以帮你？")).toBeInTheDocument(),
    );
    expect(screen.queryByText("▍")).toBeNull();

    // The IPC return commits the final turn and closes the live bubble.
    await act(async () => {
      resolveSend(
        okResult({ type: "text", content: "你好，我是糯米管家，请问有什么可以帮你？" }),
      );
    });
    await waitFor(() => expect(screen.getByText("你好")).toBeInTheDocument());
    expect(screen.queryByText("▍")).toBeNull();
  });

  it("marks the turn degraded on steward.llm_failed and offers one-click resend", async () => {
    let resolveSend: (r: Result<StewardReplyDto, IpcError>) => void = () => {};
    const send = vi.fn(
      () =>
        new Promise<Result<StewardReplyDto, IpcError>>((res) => {
          resolveSend = res;
        }),
    );
    const sid = await setupStewardSession({ send });

    await submitDraft("帮我诊断");
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));

    act(() => {
      emitTestEvent(sessionChannel(sid), {
        type: "steward.llm_failed",
        sessionId: sid,
        payload: { sessionId: sid, reason: "timeout", detail: null },
      });
    });

    await act(async () => {
      resolveSend(okResult({ type: "text", content: "模型服务响应超时，请重试。" }));
    });

    // Degrade text committed with the resend affordance (spec 5.1.3-1c).
    await waitFor(() => expect(screen.getByText("模型服务响应超时，请重试。")).toBeInTheDocument());
    const resend = await screen.findByRole("button", { name: "重发" });
    expect(resend).toBeInTheDocument();

    fireEvent.click(resend);
    await waitFor(() => expect(send).toHaveBeenCalledTimes(2));
    expect(send).toHaveBeenLastCalledWith("st-s1", "帮我诊断", null);
  });

  it("restores the input draft when the send IPC fails", async () => {
    const send = vi.fn(async () => errResult("steward.error", "boom"));
    await setupStewardSession({ send });

    const input = screen.getByPlaceholderText("向管家发送消息...") as HTMLInputElement;
    await submitDraft("帮我看看这个配置");
    await waitFor(() => expect(send).toHaveBeenCalledTimes(1));

    await screen.findByText(/发送失败/);
    // Draft preserved after the error (task 3.7: 输入草稿在错误后保留).
    await waitFor(() => expect(input.value).toBe("帮我看看这个配置"));
  });
});

describe("StewardChat v2 — attachments", () => {
  it("names the unusable attachment when the send fails validation (spec 5.3.3-1c)", async () => {
    const base = testDoubleCommands();
    const send = vi.fn(async () =>
      errResult(
        "steward.attachment_invalid",
        "附件 gone.md不可用：文件不存在，请移除后重试",
      ),
    );
    injectIpcCommands({
      ...base,
      stewardEnsureSingletonSession: vi.fn(async () => okResult(session("st-s1"))),
      stewardSaveAttachment: vi.fn(async () =>
        okResult({
          id: "att-9",
          sessionId: "st-s1",
          seq: 1,
          kind: "file",
          name: "gone.md",
          mime: "text/markdown",
          relPath: ".nuomi/attachments/st-s1/x.md",
          sizeBytes: 4,
          sha256: "x",
          createdAt: 1,
        }),
      ),
      stewardSendMessage: send,
    });
    renderWithProviders(<StewardChat />);
    await screen.findByPlaceholderText("向管家发送消息...");

    const fileInput = document.querySelector(
      'input[type="file"]',
    ) as HTMLInputElement;
    fireEvent.change(fileInput, {
      target: {
        files: [
          new File([new TextEncoder().encode("# hi")], "gone.md", {
            type: "text/markdown",
          }),
        ],
      },
    });
    await screen.findByText("gone.md");

    const input = screen.getByPlaceholderText(
      "向管家发送消息...",
    ) as HTMLInputElement;
    fireEvent.change(input, { target: { value: "总结这个文档" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    await waitFor(() =>
      expect(send).toHaveBeenCalledWith("st-s1", "总结这个文档", ["att-9"]),
    );
    await screen.findByText(/附件 gone.md不可用/);
    // Draft and attachment chip kept for removal and retry.
    await waitFor(() => expect(input.value).toBe("总结这个文档"));
    expect(screen.getByText("gone.md")).toBeInTheDocument();
  });
});

describe("StewardChat v2 — config proposal card", () => {
  const proposalReply: StewardReplyDto = {
    type: "config_proposal",
    proposal_id: "prop-1",
    diff: "--- role: role_a\n+++ role: role_a\n- temperature: 0.2\n+ temperature: 0.7",
    confirm_handle: "steward_confirm_config_change",
  };

  it("cancel keeps the config and records the cancelled state", async () => {
    const base = testDoubleCommands();
    const cancel = vi.fn(base.stewardCancelConfigChange);
    await setupStewardSession({
      send: vi.fn(async () => okResult(proposalReply)),
    });
    injectIpcCommands({ stewardCancelConfigChange: cancel });

    await submitDraft("把温度调到 0.7");
    await screen.findByText("配置变更建议");
    expect(screen.getByText(/temperature: 0.7/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    await waitFor(() => expect(cancel).toHaveBeenCalledWith("st-s1", "prop-1"));
    await screen.findByText("已取消，配置保持不变");
  });

  it("confirm applies the config change and reflects the applied state", async () => {
    const base = testDoubleCommands();
    const confirm = vi.fn(base.stewardConfirmConfigChange);
    await setupStewardSession({
      send: vi.fn(async () => okResult(proposalReply)),
    });
    injectIpcCommands({ stewardConfirmConfigChange: confirm });

    await submitDraft("把温度调到 0.7");
    await screen.findByText("配置变更建议");

    fireEvent.click(screen.getByRole("button", { name: "确认变更" }));
    await waitFor(() => expect(confirm).toHaveBeenCalledWith("prop-1"));
    await screen.findByText("已确认生效");
  });
});
