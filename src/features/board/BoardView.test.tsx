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

    renderWithProviders(<BoardView workspaceId={null} />);

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
      workspaceId: "__migrated__",
      workspaceRootPath: null,
    });
    const base = testDoubleCommands();
    const updateTaskStatus = vi.fn(base.updateTaskStatus);
    injectIpcCommands({ ...base, updateTaskStatus });

    renderWithProviders(<BoardView workspaceId={null} />);
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
      workspaceId: "__migrated__",
      workspaceRootPath: null,
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

    renderWithProviders(<BoardView workspaceId={null} />);
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
      workspaceId: "__migrated__",
      workspaceRootPath: null,
    });
    injectIpcCommands(testDoubleCommands());

    renderWithProviders(<BoardView workspaceId={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "任务操作菜单" }));
    fireEvent.click(await screen.findByRole("button", { name: "用团队运行" }));

    expect(await screen.findByText(/还没有团队/)).toBeInTheDocument();
  });
});

describe("BoardView — auto-form dry-run (打磨③b)", () => {
  const seedTask = (): void => {
    tdState.tasks.push({
      id: "t9",
      sessionId: null,
      title: "群聊任务",
      description: "覆盖三个角色",
      status: "backlog",
      createdAt: 1,
      updatedAt: 1,
      workspaceId: "__migrated__",
      workspaceRootPath: null,
    });
  };

  it("previews the plan first, then forms and runs only after confirming", async () => {
    seedTask();
    const base = testDoubleCommands();
    const previewTeam = vi.fn(base.previewTeam);
    const formTeam = vi.fn(base.formTeam);
    const runTeamOnTask = vi.fn(base.runTeamOnTask);
    injectIpcCommands({ ...base, previewTeam, formTeam, runTeamOnTask });

    renderWithProviders(<BoardView workspaceId={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "任务操作菜单" }));
    fireEvent.click(await screen.findByRole("button", { name: "自发组队运行" }));

    // Step 1: dry-run preview is issued before any dialog or side effect.
    await waitFor(() =>
      expect(previewTeam).toHaveBeenCalledWith("群聊任务\n覆盖三个角色"),
    );
    expect(formTeam).not.toHaveBeenCalled();

    // The confirmation dialog shows member rows with kind + will-create badges.
    const dialog = await screen.findByRole("dialog", { name: /自发组队预览/ });
    expect(dialog).toHaveTextContent("auto-planner");
    expect(dialog).toHaveTextContent("cli_profile");
    expect(dialog).toHaveTextContent("将新建 Role");
    expect(dialog).toHaveTextContent("auto-worker");
    expect(dialog).toHaveTextContent("群聊 Group Chat");
    expect(dialog).toHaveTextContent("规划理由");

    // Step 2: only confirming triggers the existing form → run chain.
    fireEvent.click(await screen.findByRole("button", { name: "组建并运行" }));
    await waitFor(() => expect(formTeam).toHaveBeenCalledWith("群聊任务\n覆盖三个角色", null));
    await waitFor(() => expect(runTeamOnTask).toHaveBeenCalledWith("t9", "auto-team-1"));
    const formedName = tdState.teams.find((team) => team.id === "auto-team-1")?.name;
    const runId = tdState.runs.find((run) => run.taskId === "t9")?.id;
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
              toast.message.includes(runId ?? ""),
          ),
      ).toBe(true),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("never calls formTeam when the user cancels the preview dialog", async () => {
    seedTask();
    const base = testDoubleCommands();
    const previewTeam = vi.fn(base.previewTeam);
    const formTeam = vi.fn(base.formTeam);
    const runTeamOnTask = vi.fn(base.runTeamOnTask);
    injectIpcCommands({ ...base, previewTeam, formTeam, runTeamOnTask });

    renderWithProviders(<BoardView workspaceId={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "任务操作菜单" }));
    fireEvent.click(await screen.findByRole("button", { name: "自发组队运行" }));

    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    fireEvent.click(await screen.findByRole("button", { name: "取消" }));

    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(formTeam).not.toHaveBeenCalled();
    expect(runTeamOnTask).not.toHaveBeenCalled();
    expect(tdState.teams.some((team) => team.id.startsWith("auto-team-"))).toBe(false);
  });

  it("toasts by code and skips the dialog when previewTeam fails with team.plan_invalid", async () => {
    seedTask();
    const base = testDoubleCommands();
    const previewTeam = vi.fn(async () => {
      throw new IpcCommandError("team.plan_invalid", "plan invalid");
    });
    const formTeam = vi.fn(base.formTeam);
    const runTeamOnTask = vi.fn(base.runTeamOnTask);
    injectIpcCommands({ ...base, previewTeam, formTeam, runTeamOnTask });

    renderWithProviders(<BoardView workspaceId={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "任务操作菜单" }));
    fireEvent.click(await screen.findByRole("button", { name: "自发组队运行" }));

    await waitFor(() => expect(previewTeam).toHaveBeenCalledOnce());
    await waitFor(() =>
      expect(
        useToastStore
          .getState()
          .toasts.some((toast) => toast.kind === "error" && toast.message.includes("组队计划无效")),
      ).toBe(true),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(formTeam).not.toHaveBeenCalled();
    expect(runTeamOnTask).not.toHaveBeenCalled();
  });
});

describe("BoardView — batch operations (批次二②)", () => {
  const seedTask = (id: string, title: string, status: string): void => {
    tdState.tasks.push({
      id,
      sessionId: null,
      title,
      description: "",
      status,
      createdAt: 1,
      updatedAt: 1,
      workspaceId: "__migrated__",
      workspaceRootPath: null,
    });
  };

  it("deletes a task after the two-step confirm and refreshes the board", async () => {
    seedTask("t-del", "待删任务", "backlog");
    tdState.runs.push({ id: "r-del", taskId: "t-del", sessionId: "s1", status: "failed", heartbeatAt: 1, kind: "task", cancelable: false });
    const base = testDoubleCommands();
    const deleteTask = vi.fn(base.deleteTask);
    injectIpcCommands({ ...base, deleteTask });

    renderWithProviders(<BoardView workspaceId={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "任务操作菜单" }));

    // Step one only arms the confirm — no IPC call yet.
    fireEvent.click(screen.getByRole("button", { name: "删除 待删任务" }));
    expect(deleteTask).not.toHaveBeenCalled();

    // Step two deletes, toasts, and drops the card (runs cache invalidated too).
    fireEvent.click(await screen.findByRole("button", { name: "确认删除 待删任务" }));
    await waitFor(() => expect(deleteTask).toHaveBeenCalledWith("t-del"));
    await waitFor(() =>
      expect(
        useToastStore
          .getState()
          .toasts.some((toast) => toast.kind === "success" && toast.message.includes("任务已删除")),
      ).toBe(true),
    );
    await waitFor(() => expect(screen.queryByText("待删任务")).not.toBeInTheDocument());
    expect(tdState.tasks.some((task) => task.id === "t-del")).toBe(false);
    expect(tdState.runs.some((run) => run.id === "r-del")).toBe(false);
  });

  it("hides the delete item while a card is running", async () => {
    seedTask("t-run", "运行中任务", "running");
    injectIpcCommands(testDoubleCommands());

    renderWithProviders(<BoardView workspaceId={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "任务操作菜单" }));

    expect(screen.queryByRole("button", { name: /^删除/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^确认删除/ })).not.toBeInTheDocument();
  });

  it("dispatches every queued task sequentially after confirmation and toasts the summary", async () => {
    seedTask("t1", "排队一", "queued");
    seedTask("t2", "排队二", "queued");
    seedTask("t3", "排队三", "queued");
    const base = testDoubleCommands();
    const updateTaskStatus = vi.fn(base.updateTaskStatus);
    injectIpcCommands({ ...base, updateTaskStatus });

    renderWithProviders(<BoardView workspaceId={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "运行全部" }));
    fireEvent.click(await screen.findByRole("button", { name: "确认派发 3 个排队任务？" }));

    await waitFor(() => expect(updateTaskStatus).toHaveBeenCalledTimes(3));
    expect(updateTaskStatus.mock.calls).toEqual([
      ["t1", "running"],
      ["t2", "running"],
      ["t3", "running"],
    ]);
    await waitFor(() =>
      expect(
        useToastStore
          .getState()
          .toasts.some((toast) => toast.kind === "success" && toast.message.includes("已派发 3/3")),
      ).toBe(true),
    );
    expect(tdState.tasks.every((task) => task.status === "running")).toBe(true);
  });

  it("dispatches nothing when the run-all confirm is cancelled", async () => {
    seedTask("t1", "排队一", "queued");
    const base = testDoubleCommands();
    const updateTaskStatus = vi.fn(base.updateTaskStatus);
    injectIpcCommands({ ...base, updateTaskStatus });

    renderWithProviders(<BoardView workspaceId={null} />);
    fireEvent.click(await screen.findByRole("button", { name: "运行全部" }));
    fireEvent.click(await screen.findByRole("button", { name: "取消" }));

    expect(updateTaskStatus).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: /确认派发/ })).not.toBeInTheDocument();
  });

  it("keeps run all disabled on an empty queued column", async () => {
    // A non-queued card keeps the board out of its empty state while the
    // queued column stays at N=0.
    seedTask("t-bl", "待办任务", "backlog");
    injectIpcCommands(testDoubleCommands());

    renderWithProviders(<BoardView workspaceId={null} />);

    const button = await screen.findByRole("button", { name: "运行全部" });
    expect(button).toBeDisabled();
  });
});

describe("BoardView — view scope toggle (task 7.3)", () => {
  it("renders the scope toggle and calls setViewScope on click", async () => {
    const base = testDoubleCommands();
    const setViewScope = vi.fn(base.setViewScope);
    injectIpcCommands({ ...base, setViewScope });

    renderWithProviders(<BoardView workspaceId={null} />);

    const focusedBtn = await screen.findByText("仅看聚焦");
    fireEvent.click(focusedBtn);
    await waitFor(() => expect(setViewScope).toHaveBeenCalledWith("board", "focused"));
  });
});
