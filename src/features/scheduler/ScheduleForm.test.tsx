import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { seedRole, tdState, testDoubleCommands } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { useUiStore } from "../../lib/store/uiStore";
import { ScheduleForm } from "./ScheduleForm";

function setup(): ReturnType<typeof vi.fn> {
  const base = testDoubleCommands();
  const upsertSchedule = vi.fn(base.upsertSchedule);
  injectIpcCommands({ ...base, upsertSchedule });
  useUiStore.setState({ activeWorkspaceId: "ws-1" });
  return upsertSchedule;
}

function fillBasics(): void {
  fireEvent.change(screen.getByLabelText("名称"), { target: { value: "nightly" } });
  fireEvent.change(screen.getByLabelText("Cron 表达式"), { target: { value: "0 9 * * *" } });
  fireEvent.change(screen.getByLabelText("任务标题"), { target: { value: "build" } });
}

describe("ScheduleForm — full schedule surface (P2-F 组 1)", () => {
  it("submits a task schedule with defaults", async () => {
    const upsertSchedule = setup();
    const onDone = vi.fn();
    renderWithProviders(<ScheduleForm onDone={onDone} />);

    fillBasics();
    fireEvent.click(screen.getByRole("button", { name: "创建" }));

    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(upsertSchedule).toHaveBeenCalledWith({
      name: "nightly",
      cronExpr: "0 9 * * *",
      targetKind: "task",
      agent: null,
      teamId: null,
      sessionMode: "per_trigger",
      sessionId: null,
      autoDispatch: true,
      taskTitle: "build",
      taskDescription: "",
    });
  });

  it("submits a group schedule with spontaneous formation (no team)", async () => {
    const upsertSchedule = setup();
    renderWithProviders(<ScheduleForm onDone={vi.fn()} />);

    fillBasics();
    fireEvent.click(screen.getByRole("button", { name: "群聊" }));
    fireEvent.click(screen.getByRole("button", { name: "创建" }));

    await waitFor(() => expect(upsertSchedule).toHaveBeenCalled());
    expect(upsertSchedule.mock.calls.at(0)?.[0]).toMatchObject({ targetKind: "group", teamId: null });
  });

  it("submits a group schedule with a pre-selected team", async () => {
    const upsertSchedule = setup();
    tdState.teams.push({
      id: "team-1",
      name: "review-crew",
      topology: "group_chat",
      memberRoleIds: [],
      config: {},
      createdAt: 0,
      updatedAt: 0,
    });
    renderWithProviders(<ScheduleForm onDone={vi.fn()} />);

    fillBasics();
    fireEvent.click(screen.getByRole("button", { name: "群聊" }));
    fireEvent.change(await screen.findByLabelText("团队来源"), { target: { value: "pick" } });
    fireEvent.change(await screen.findByLabelText("预选团队"), { target: { value: "team-1" } });
    fireEvent.click(screen.getByRole("button", { name: "创建" }));

    await waitFor(() => expect(upsertSchedule).toHaveBeenCalled());
    expect(upsertSchedule.mock.calls.at(0)?.[0]).toMatchObject({ targetKind: "group", teamId: "team-1" });
  });

  it("submits a chat schedule with the selected role agent and autoDispatch off", async () => {
    const upsertSchedule = setup();
    seedRole({ id: "role-1", name: "researcher", providerId: "p1" });
    renderWithProviders(<ScheduleForm onDone={vi.fn()} />);

    fillBasics();
    fireEvent.click(screen.getByRole("button", { name: "单聊" }));
    await screen.findByText("researcher");
    fireEvent.change(screen.getByLabelText("对话角色"), { target: { value: "role-1" } });
    fireEvent.click(screen.getByLabelText("触发后立即执行"));
    fireEvent.click(screen.getByRole("button", { name: "创建" }));

    await waitFor(() => expect(upsertSchedule).toHaveBeenCalled());
    expect(upsertSchedule.mock.calls.at(0)?.[0]).toMatchObject({
      targetKind: "chat",
      agent: { kind: "role", id: "role-1" },
      autoDispatch: false,
    });
  });

  it("blocks chat submission without a role agent", async () => {
    const upsertSchedule = setup();
    const onDone = vi.fn();
    renderWithProviders(<ScheduleForm onDone={onDone} />);

    fillBasics();
    fireEvent.click(screen.getByRole("button", { name: "单聊" }));
    fireEvent.click(screen.getByRole("button", { name: "创建" }));

    await screen.findByLabelText("对话角色");
    expect(upsertSchedule).not.toHaveBeenCalled();
    expect(onDone).not.toHaveBeenCalled();
  });
});
