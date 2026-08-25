import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { tdState, testDoubleCommands } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { BoardView } from "./BoardView";

describe("BoardView — create task flow (AC10)", () => {
  it("calls createTask and refreshes the board list", async () => {
    const base = testDoubleCommands();
    const createTask = vi.fn(base.createTask);
    injectIpcCommands({ ...base, createTask });

    renderWithProviders(<BoardView />);

    fireEvent.click(await screen.findByRole("button", { name: "新建任务" }));
    fireEvent.change(screen.getByLabelText("标题"), { target: { value: "重构内核" } });
    fireEvent.change(screen.getByLabelText("描述"), { target: { value: "拆分 store 层" } });
    fireEvent.click(screen.getByRole("button", { name: "创建" }));

    await waitFor(() => expect(createTask).toHaveBeenCalledWith("重构内核", "拆分 store 层"));
    // List refresh: the new card shows up in its column.
    await screen.findByText("重构内核");
    expect(tdState.tasks[0]?.status).toBe("backlog");
  });

  it("moves a task via the equivalent keyboard menu", async () => {
    tdState.tasks.push({
      id: "t1",
      sessionId: null,
      title: "写文档",
      description: "",
      status: "backlog",
      createdAt: 1,
      updatedAt: 1,
    });
    const base = testDoubleCommands();
    const updateTaskStatus = vi.fn(base.updateTaskStatus);
    injectIpcCommands({ ...base, updateTaskStatus });

    renderWithProviders(<BoardView />);
    fireEvent.click(await screen.findByRole("button", { name: "任务操作菜单" }));
    fireEvent.click(await screen.findByRole("button", { name: "移动到 运行中" }));

    await waitFor(() => expect(updateTaskStatus).toHaveBeenCalledWith("t1", "running"));
    await screen.findByText("写文档"); // still rendered after refresh
  });
});
