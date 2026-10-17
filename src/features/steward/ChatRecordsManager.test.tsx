import { screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { testDoubleCommands } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { useToastStore } from "../../lib/store/toastStore";
import { ChatRecordsManager } from "./ChatRecordsManager";
import type { MessageStatsDto, StewardMessageDto } from "../../lib/ipc/bindings.gen";

const emptyStats: MessageStatsDto = { total: 0, clearedAt: null };
const statsWithMessages: MessageStatsDto = { total: 3, clearedAt: null };

const sampleMessages: StewardMessageDto[] = [
  { id: "evt-1", role: "user", text: "Hello", createdAt: 1000 },
  { id: "evt-2", role: "steward", text: "Hi there", createdAt: 2000 },
  { id: "evt-3", role: "user", text: "How are you?", createdAt: 3000 },
];

describe("ChatRecordsManager — chat records management", () => {
  it("shows '暂无消息' when no messages", () => {
    renderWithProviders(
      <ChatRecordsManager sessionId="s1" messageStats={emptyStats} messages={[]} />,
    );
    expect(screen.getByText("暂无消息")).toBeTruthy();
  });

  it("shows message count", () => {
    renderWithProviders(
      <ChatRecordsManager sessionId="s1" messageStats={statsWithMessages} messages={sampleMessages} />,
    );
    expect(screen.getByText("3 条消息")).toBeTruthy();
  });

  it("shows '清空记录' and '批量删除' buttons when messages exist", () => {
    renderWithProviders(
      <ChatRecordsManager sessionId="s1" messageStats={statsWithMessages} messages={sampleMessages} />,
    );
    expect(screen.getByText("清空记录")).toBeTruthy();
    expect(screen.getByText("批量删除")).toBeTruthy();
  });

  it("does not show action buttons when no messages", () => {
    renderWithProviders(
      <ChatRecordsManager sessionId="s1" messageStats={emptyStats} messages={[]} />,
    );
    expect(screen.queryByText("清空记录")).toBeNull();
    expect(screen.queryByText("批量删除")).toBeNull();
  });

  it("opens clear confirmation dialog on '清空记录' click", () => {
    renderWithProviders(
      <ChatRecordsManager sessionId="s1" messageStats={statsWithMessages} messages={sampleMessages} />,
    );
    fireEvent.click(screen.getByText("清空记录"));
    expect(screen.getByText("此操作不可撤销")).toBeTruthy();
  });

  it("calls stewardClearMessages on confirm", async () => {
    const base = testDoubleCommands();
    const clearMessages = vi.fn(base.stewardClearMessages);
    injectIpcCommands({ ...base, stewardClearMessages: clearMessages });

    renderWithProviders(
      <ChatRecordsManager sessionId="s1" messageStats={statsWithMessages} messages={sampleMessages} />,
    );
    fireEvent.click(screen.getByText("清空记录"));
    fireEvent.click(screen.getByText("确认"));

    await waitFor(() => expect(clearMessages).toHaveBeenCalledWith("s1"));
  });

  it("toasts success when messages are cleared", async () => {
    const base = testDoubleCommands();
    const clearMessages = vi.fn(base.stewardClearMessages);
    injectIpcCommands({ ...base, stewardClearMessages: clearMessages });

    renderWithProviders(
      <ChatRecordsManager sessionId="s1" messageStats={statsWithMessages} messages={sampleMessages} />,
    );
    fireEvent.click(screen.getByText("清空记录"));
    fireEvent.click(screen.getByText("确认"));

    await waitFor(() =>
      expect(useToastStore.getState().toasts).toHaveLength(1),
    );
    expect(useToastStore.getState().toasts[0]).toMatchObject({
      kind: "success",
      message: "聊天记录已清空",
    });
  });

  it("toasts failure and keeps the confirm dialog flow usable when clear fails", async () => {
    const base = testDoubleCommands();
    const clearMessages = vi.fn(async () => {
      throw new Error("storage locked");
    });
    injectIpcCommands({ ...base, stewardClearMessages: clearMessages });

    renderWithProviders(
      <ChatRecordsManager sessionId="s1" messageStats={statsWithMessages} messages={sampleMessages} />,
    );
    fireEvent.click(screen.getByText("清空记录"));
    fireEvent.click(screen.getByText("确认"));

    await waitFor(() =>
      expect(useToastStore.getState().toasts).toHaveLength(1),
    );
    expect(useToastStore.getState().toasts[0]?.kind).toBe("error");
    expect(useToastStore.getState().toasts[0]?.message).toContain("清空失败");
  });

  it("enters select mode on '批量删除' click", () => {
    renderWithProviders(
      <ChatRecordsManager sessionId="s1" messageStats={statsWithMessages} messages={sampleMessages} />,
    );
    fireEvent.click(screen.getByText("批量删除"));
    expect(screen.getByText(/删除选中/)).toBeTruthy();
    expect(screen.getByText("取消")).toBeTruthy();
  });

  it("exits select mode on '取消' click", () => {
    renderWithProviders(
      <ChatRecordsManager sessionId="s1" messageStats={statsWithMessages} messages={sampleMessages} />,
    );
    fireEvent.click(screen.getByText("批量删除"));
    fireEvent.click(screen.getByText("取消"));
    expect(screen.queryByText(/删除选中/)).toBeNull();
    expect(screen.getByText("批量删除")).toBeTruthy();
  });

  it("disables delete button when no messages selected", () => {
    renderWithProviders(
      <ChatRecordsManager sessionId="s1" messageStats={statsWithMessages} messages={sampleMessages} />,
    );
    fireEvent.click(screen.getByText("批量删除"));
    const deleteBtn = screen.getByText(/删除选中/);
    expect((deleteBtn as HTMLButtonElement).disabled).toBe(true);
  });

  it("calls stewardBatchDeleteMessages with selected ids", async () => {
    const base = testDoubleCommands();
    const batchDelete = vi.fn(base.stewardBatchDeleteMessages);
    injectIpcCommands({ ...base, stewardBatchDeleteMessages: batchDelete });

    renderWithProviders(
      <ChatRecordsManager sessionId="s1" messageStats={statsWithMessages} messages={sampleMessages} />,
    );
    fireEvent.click(screen.getByText("批量删除"));

    const checkboxes = screen.getAllByRole("checkbox");
    fireEvent.click(checkboxes[0]!);

    fireEvent.click(screen.getByText(/删除选中/));

    await waitFor(() =>
      expect(batchDelete).toHaveBeenCalledWith("s1", ["evt-1"]),
    );
  });
});
