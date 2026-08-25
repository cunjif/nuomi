import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands, IpcCommandError } from "../../lib/ipc/client";
import { useToastStore } from "../../lib/store/toastStore";
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

describe("BoardView — run task with team (M-TEAM1 T5)", () => {
  it("starts a team run from the card menu and toasts the run id", async () => {
    tdState.tasks.push({
      id: "t9",
      sessionId: null,
      title: "群聊任务",
      description: "",
      status: "backlog",
      createdAt: 1,
      updatedAt: 1,
    });
    tdState.teams.push({
      id: "team-1",
      name: "梦之队",
      topology: "group_chat",
      memberRoleIds: [],
      config: {},
      createdAt: 1,
      updatedAt: 1,
    });
    const base = testDoubleCommands();
    const runTeamOnTask = vi.fn(base.runTeamOnTask);
    injectIpcCommands({ ...base, runTeamOnTask });

    renderWithProviders(<BoardView />);
    fireEvent.click(await screen.findByRole("button", { name: "任务操作菜单" }));
    fireEvent.click(await screen.findByRole("button", { name: "用团队运行" }));
    fireEvent.click(await screen.findByRole("button", { name: "梦之队" }));

    await waitFor(() => expect(runTeamOnTask).toHaveBeenCalledWith("t9", "team-1"));
    await waitFor(() =>
      expect(
        useToastStore
          .getState()
          .toasts.some((toast) => toast.message.includes("团队运行已启动"))
      ).toBe(true)
    );
    // The deterministic double settled the run into the task's run list.
    expect(tdState.runs.some((run) => run.taskId === "t9" && run.status === "succeeded")).toBe(true);
  });

  it("shows the empty-team hint when no team exists", async () => {
    tdState.tasks.push({
      id: "t10",
      sessionId: null,
      title: "无团队任务",
      description: "",
      status: "backlog",
      createdAt: 1,
      updatedAt: 1,
    });
    injectIpcCommands(testDoubleCommands());

    renderWithProviders(<BoardView />);
    fireEvent.click(await screen.findByRole("button", { name: "任务操作菜单" }));
    fireEvent.click(await screen.findByRole("button", { name: "用团队运行" }));

    expect(await screen.findByText(/还没有团队/)).toBeInTheDocument();
  });
});

describe("BoardView — auto-form & run (M-FORM1 F4)", () => {
  it("forms a team from the task text, then runs it and toasts team + run id", async () => {
    tdState.tasks.push({
      id: "t9",
      sessionId: null,
      title: "群聊任务",
      description: "覆盖三个角色",
      status: "backlog",
      createdAt: 1,
      updatedAt: 1,
    });
    const base = testDoubleCommands();
    const formTeam = vi.fn(base.formTeam);
    const runTeamOnTask = vi.fn(base.runTeamOnTask);
    injectIpcCommands({ ...base, formTeam, runTeamOnTask });

    renderWithProviders(<BoardView />);
    fireEvent.click(await screen.findByRole("button", { name: "任务操作菜单" }));
    fireEvent.click(await screen.findByRole("button", { name: "自发组队运行" }));

    await waitFor(() => expect(formTeam).toHaveBeenCalledWith("群聊任务\n覆盖三个角色", null));
    await waitFor(() => expect(runTeamOnTask).toHaveBeenCalledWith("t9", "auto-team-1"));
    // The formed team landed in state as a runnable group_chat team.
    const formedName = tdState.teams.find((t) => t.id === "auto-team-1")?.name;
    expect(formedName).toBeDefined();
    expect(tdState.teams.some((t) => t.id === "auto-team-1" && t.topology === "group_chat")).toBe(
      true,
    );
    const runId = tdState.runs.find((r) => r.taskId === "t9")?.id;
    expect(runId).toBeDefined();
    await waitFor(() =>
      expect(
        useToastStore
          .getState()
          .toasts.some(
            (toast) =>
              toast.kind === "success" &&
              toast.message.includes("自发组队完成") &&
              toast.message.includes(formedName ?? "") &&
              toast.message.includes("2 名成员") &&
              toast.message.includes(runId ?? ""),
          ),
      ).toBe(true),
    );
  });

  it("does not start a run when formTeam fails with team.plan_invalid", async () => {
    tdState.tasks.push({
      id: "t11",
      sessionId: null,
      title: "坏计划任务",
      description: "",
      status: "backlog",
      createdAt: 1,
      updatedAt: 1,
    });
    const base = testDoubleCommands();
    const formTeam = vi.fn(async () => {
      throw new IpcCommandError("team.plan_invalid", "plan invalid");
    });
    const runTeamOnTask = vi.fn(base.runTeamOnTask);
    injectIpcCommands({ ...base, formTeam, runTeamOnTask });

    renderWithProviders(<BoardView />);
    fireEvent.click(await screen.findByRole("button", { name: "任务操作菜单" }));
    fireEvent.click(await screen.findByRole("button", { name: "自发组队运行" }));

    await waitFor(() => expect(formTeam).toHaveBeenCalledOnce());
    await waitFor(() =>
      expect(
        useToastStore
          .getState()
          .toasts.some((toast) => toast.kind === "error" && toast.message.includes("组队计划无效")),
      ).toBe(true),
    );
    // Step two never fired on a failed form.
    expect(runTeamOnTask).not.toHaveBeenCalled();
    expect(tdState.teams.some((t) => t.id.startsWith("auto-team-"))).toBe(false);
  });
});
