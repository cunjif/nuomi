import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { testDoubleCommands } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { Toaster } from "../../components/ui/Toaster";
import type {
  IpcError,
  PromptOptimizeResultDto,
  Result,
  StewardAttachmentDto,
} from "../../lib/ipc/bindings.gen";
import { StewardComposer } from "./StewardComposer";

const okResult = <T,>(data: T): Result<T, IpcError> => ({ status: "ok", data });
const errResult = (code: string, message: string): Result<never, IpcError> => ({
  status: "error",
  error: { generic: { code, message } },
});

const attachmentDto = (id: string, name: string): StewardAttachmentDto => ({
  id,
  sessionId: "st-s1",
  seq: 1,
  kind: "image",
  name,
  mime: "image/png",
  relPath: `.nuomi/attachments/st-s1/${id}.png`,
  sizeBytes: 8,
  sha256: id,
  createdAt: 1,
});

interface SetupOverrides {
  save?: (
    sessionId: string,
    name: string,
    mime: string,
    base64: string,
  ) => Promise<Result<StewardAttachmentDto, IpcError>>;
  optimize?: (
    sessionId: string,
    draft: string,
  ) => Promise<Result<PromptOptimizeResultDto, IpcError>>;
  onSend?: (text: string, attachmentIds: string[]) => Promise<boolean>;
}

async function setup(overrides?: SetupOverrides): Promise<{
  saveMock: ReturnType<typeof vi.fn>;
  optimizeMock: ReturnType<typeof vi.fn>;
  onSendMock: ReturnType<typeof vi.fn>;
  pickFiles: (files: File[]) => void;
}> {
  const base = testDoubleCommands();
  const saveMock = vi.fn(
    overrides?.save ??
      (async () => okResult(attachmentDto("att-1", "shot.png"))),
  );
  const optimizeMock = vi.fn(
    overrides?.optimize ??
      (async (_s: string, draft: string) =>
        okResult({ optimized: `[优化] ${draft}` })),
  );
  const onSendMock = vi.fn(overrides?.onSend ?? (async () => true));
  injectIpcCommands({
    ...base,
    stewardSaveAttachment: saveMock,
    optimizePrompt: optimizeMock,
  });
  const { container } = renderWithProviders(
    <>
      <StewardComposer sessionId="st-s1" disabled={false} onSend={onSendMock} />
      <Toaster />
    </>,
  );
  await screen.findByPlaceholderText("向管家发送消息...");
  const pickFiles = (files: File[]): void => {
    const input = container.querySelector(
      'input[type="file"]',
    ) as HTMLInputElement;
    fireEvent.change(input, { target: { files } });
  };
  return { saveMock, optimizeMock, onSendMock, pickFiles };
}

const pngFile = (name = "shot.png"): File =>
  new File([new Uint8Array([0x89, 0x50, 0x4e, 0x47])], name, { type: "image/png" });

const txtFile = (name = "notes.txt"): File =>
  new File([new TextEncoder().encode("hello")], name, { type: "text/plain" });

function draftInput(): HTMLTextAreaElement {
  return screen.getByPlaceholderText("向管家发送消息...") as HTMLTextAreaElement;
}

describe("StewardComposer — attachment preview", () => {
  it("saves a picked image and renders a removable thumbnail", async () => {
    const { saveMock, pickFiles } = await setup();
    pickFiles([pngFile()]);

    await waitFor(() =>
      expect(saveMock).toHaveBeenCalledWith(
        "st-s1",
        "shot.png",
        "image/png",
        expect.any(String),
      ),
    );
    const thumb = await screen.findByAltText("shot.png");
    expect(thumb).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "移除附件" }));
    expect(screen.queryByAltText("shot.png")).toBeNull();
  });

  it("renders a document chip without a thumbnail for non-image files", async () => {
    const { saveMock, pickFiles } = await setup({
      save: vi.fn(async () =>
        okResult({ ...attachmentDto("att-2", "notes.txt"), kind: "file", mime: "text/plain" }),
      ),
    });
    pickFiles([txtFile()]);

    await waitFor(() => expect(saveMock).toHaveBeenCalledTimes(1));
    await screen.findByText("notes.txt");
    expect(screen.queryByRole("img")).toBeNull();
  });
});

describe("StewardComposer — validation notices (toast)", () => {
  it("rejects a non-whitelisted type without saving", async () => {
    const { saveMock, pickFiles } = await setup();
    pickFiles([
      new File([new Uint8Array([0])], "tool.exe", {
        type: "application/x-msdownload",
      }),
    ]);

    const toast = await screen.findByText(/不支持的文件类型/);
    expect(toast.textContent).toContain("tool.exe");
    expect(saveMock).not.toHaveBeenCalled();
  });

  it("rejects an oversized file without saving", async () => {
    const { saveMock, pickFiles } = await setup();
    pickFiles([
      new File([new Uint8Array(11 * 1024 * 1024)], "big.png", { type: "image/png" }),
    ]);

    const toast = await screen.findByText(/文件过大/);
    expect(toast.textContent).toContain("big.png");
    expect(saveMock).not.toHaveBeenCalled();
  });

  it("surfaces a backend save failure naming the attachment", async () => {
    const { pickFiles } = await setup({
      save: vi.fn(async () =>
        errResult("steward.attachment_too_large", "附件 big.png 超过大小上限"),
      ),
    });
    pickFiles([pngFile("big.png")]);

    const toast = await screen.findByText(/附件保存失败/);
    expect(toast.textContent).toContain("big.png");
    expect(screen.queryByAltText("big.png")).toBeNull();
  });
});

describe("StewardComposer — send flow", () => {
  it("sends attachment ids with the draft and clears on success", async () => {
    const { onSendMock, pickFiles } = await setup();
    pickFiles([pngFile()]);

    const input = draftInput();
    await screen.findByAltText("shot.png");
    fireEvent.change(input, { target: { value: "总结这个文档" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    await waitFor(() =>
      expect(onSendMock).toHaveBeenCalledWith("总结这个文档", ["att-1"]),
    );
    await waitFor(() => expect(input.value).toBe(""));
    expect(screen.queryByAltText("shot.png")).toBeNull();
  });

  it("keeps the draft and attachments when the send fails", async () => {
    const { onSendMock, pickFiles } = await setup({
      onSend: vi.fn(async () => false),
    });
    pickFiles([pngFile()]);

    const input = draftInput();
    await screen.findByAltText("shot.png");
    fireEvent.change(input, { target: { value: "这张图里有什么" } });
    fireEvent.click(screen.getByRole("button", { name: "发送" }));

    await waitFor(() => expect(onSendMock).toHaveBeenCalledTimes(1));
    expect(input.value).toBe("这张图里有什么");
    expect(screen.getByAltText("shot.png")).toBeInTheDocument();
  });

  it("submits on Enter and inserts a newline on Shift+Enter (unified composer contract)", async () => {
    const { onSendMock } = await setup();
    const input = draftInput();

    fireEvent.change(input, { target: { value: "第一行" } });
    fireEvent.keyDown(input, { key: "Enter", shiftKey: true });
    expect(onSendMock).not.toHaveBeenCalled();

    fireEvent.keyDown(input, { key: "Enter", shiftKey: false });
    await waitFor(() =>
      expect(onSendMock).toHaveBeenCalledWith("第一行", []),
    );
  });
});

describe("StewardComposer — AI optimize (task 4.7, session-generic IPC)", () => {
  async function draft(text: string): Promise<HTMLTextAreaElement> {
    const input = draftInput();
    fireEvent.change(input, { target: { value: text } });
    return input;
  }

  it("shows the original-vs-optimized compare and adopts the rewrite", async () => {
    await setup();
    const input = await draft("帮我诊断");

    fireEvent.click(screen.getByRole("button", { name: "AI 优化提示词" }));

    const dialog = await screen.findByRole("dialog", { name: "提示词优化对比" });
    expect(dialog.textContent).toContain("原文");
    expect(dialog.textContent).toContain("帮我诊断");
    expect(dialog.textContent).toContain("[优化] 帮我诊断");
    // Draft is untouched before 采纳 (spec 5.3.1-4: no implicit overwrite).
    expect(input.value).toBe("帮我诊断");

    fireEvent.click(screen.getByRole("button", { name: "采纳" }));
    await waitFor(() => expect(input.value).toBe("[优化] 帮我诊断"));
    expect(
      screen.queryByRole("dialog", { name: "提示词优化对比" }),
    ).toBeNull();
  });

  it("re-optimize reruns against the original draft", async () => {
    const { optimizeMock } = await setup();
    await draft("帮我诊断");
    fireEvent.click(screen.getByRole("button", { name: "AI 优化提示词" }));
    await screen.findByRole("dialog", { name: "提示词优化对比" });

    fireEvent.click(screen.getByRole("button", { name: "重新优化" }));

    await waitFor(() => expect(optimizeMock).toHaveBeenCalledTimes(2));
    expect(optimizeMock).toHaveBeenLastCalledWith("st-s1", "帮我诊断");
    await screen.findByRole("dialog", { name: "提示词优化对比" });
  });

  it("discard keeps the original draft", async () => {
    await setup();
    const input = await draft("帮我诊断");
    fireEvent.click(screen.getByRole("button", { name: "AI 优化提示词" }));
    await screen.findByRole("dialog", { name: "提示词优化对比" });

    fireEvent.click(screen.getByRole("button", { name: "放弃" }));

    expect(
      screen.queryByRole("dialog", { name: "提示词优化对比" }),
    ).toBeNull();
    expect(input.value).toBe("帮我诊断");
  });

  it("surfaces the timeout error and keeps the draft (spec 5.3.3-3c)", async () => {
    await setup({
      optimize: vi.fn(async () =>
        errResult("prompt.optimize_timeout", "提示词优化超时，请重试"),
      ),
    });
    const input = await draft("帮我诊断");
    fireEvent.click(screen.getByRole("button", { name: "AI 优化提示词" }));

    const toast = await screen.findByText(/提示词优化超时/);
    expect(toast).toBeInTheDocument();
    expect(input.value).toBe("帮我诊断");
    expect(
      screen.queryByRole("dialog", { name: "提示词优化对比" }),
    ).toBeNull();
  });
});
