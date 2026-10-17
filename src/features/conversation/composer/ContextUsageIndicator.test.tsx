import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../../lib/ipc/client";
import { testDoubleCommands } from "../../../lib/ipc/test-double";
import { renderWithProviders } from "../../../test/helpers";
import type { ContextUsageDto, IpcError, Result } from "../../../lib/ipc/bindings.gen";
import { ContextUsageIndicator } from "./ContextUsageIndicator";
import { scanRawPaletteClasses } from "../../../test/theme-tokens";

const okResult = <T,>(data: T): Result<T, IpcError> => ({ status: "ok", data });

const usage = (overrides?: Partial<ContextUsageDto>): ContextUsageDto => ({
  sections: [
    { name: "SystemPrompt", tokens: 120 },
    { name: "History", tokens: 80 },
    { name: "ToolResults", tokens: 40 },
  ],
  totalTokens: 240,
  budgetTokens: 32768,
  pruneRecords: [
    {
      occurredAt: 1760000000000,
      budgetTokens: 32768,
      approxTokens: 31000,
      duplicatesCollapsed: 2,
      resultsSummarized: 3,
      foldedMessages: 5,
    },
  ],
  ...overrides,
});

function setup(data: ContextUsageDto): ReturnType<typeof renderWithProviders> {
  const base = testDoubleCommands();
  injectIpcCommands({
    ...base,
    getContextUsage: vi.fn(async () => okResult(data)),
  });
  return renderWithProviders(<ContextUsageIndicator sessionId="s1" />);
}

describe("ContextUsageIndicator — ratio bar", () => {
  it("renders the usage ratio bar with total and budget", async () => {
    setup(usage());

    const btn = await screen.findByRole("button", { name: "上下文用量" });
    expect(btn.textContent).toContain("240");
    expect(btn.textContent).toContain("32.8k");
  });

  it("renders nothing without a session", () => {
    const base = testDoubleCommands();
    injectIpcCommands(base);
    renderWithProviders(<ContextUsageIndicator sessionId={null} />);
    expect(screen.queryByRole("button", { name: "上下文用量" })).toBeNull();
  });

  it("uses only semantic theme tokens in bar and popup (no raw palette classes)", async () => {
    const { container } = setup(usage());

    await screen.findByRole("button", { name: "上下文用量" });
    expect(scanRawPaletteClasses(container)).toEqual([]);

    fireEvent.click(screen.getByRole("button", { name: "上下文用量" }));
    await screen.findByRole("dialog", { name: "Context 压缩工作浮窗" });
    expect(scanRawPaletteClasses(container)).toEqual([]);
  });
});

describe("ContextUsageIndicator — prune popup", () => {
  it("opens on click showing sections, ratio and prune records", async () => {
    setup(usage());

    fireEvent.click(await screen.findByRole("button", { name: "上下文用量" }));

    const dialog = await screen.findByRole("dialog", { name: "Context 压缩工作浮窗" });
    expect(dialog).toBeInTheDocument();
    expect(screen.getByText("SystemPrompt")).toBeInTheDocument();
    expect(screen.getByText("History")).toBeInTheDocument();
    expect(screen.getByText("ToolResults")).toBeInTheDocument();
    expect(screen.getByText(/总用量 240 \/ 32768 tokens/)).toBeInTheDocument();
    // Prune record: when it happened and what it collapsed (spec 5.3.1-3b).
    expect(screen.getByText(/折叠 5 条/)).toBeInTheDocument();
    expect(screen.getByText(/去重 2 条/)).toBeInTheDocument();
    expect(screen.getByText(/摘要 3 条结果/)).toBeInTheDocument();
  });

  it("shows an empty-state note when there are no prune records", async () => {
    setup(usage({ pruneRecords: [] }));

    fireEvent.click(await screen.findByRole("button", { name: "上下文用量" }));
    await screen.findByRole("dialog", { name: "Context 压缩工作浮窗" });
    expect(screen.getByText("暂无压缩记录")).toBeInTheDocument();
  });

  it("closes on outside click without residual state (spec 5.3.1-3c)", async () => {
    setup(usage());

    fireEvent.click(await screen.findByRole("button", { name: "上下文用量" }));
    await screen.findByRole("dialog", { name: "Context 压缩工作浮窗" });

    fireEvent.mouseDown(document.body);
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "Context 压缩工作浮窗" }),
      ).toBeNull(),
    );

    // Re-open works cleanly after an outside-close.
    fireEvent.click(screen.getByRole("button", { name: "上下文用量" }));
    await screen.findByRole("dialog", { name: "Context 压缩工作浮窗" });
  });

  it("keeps the popup open for inside clicks", async () => {
    setup(usage());

    fireEvent.click(await screen.findByRole("button", { name: "上下文用量" }));
    const dialog = await screen.findByRole("dialog", { name: "Context 压缩工作浮窗" });

    fireEvent.mouseDown(dialog.firstElementChild ?? dialog);
    expect(
      screen.getByRole("dialog", { name: "Context 压缩工作浮窗" }),
    ).toBeInTheDocument();
  });
});
