/**
 * RunDrawer tests: task title in the header (not the raw id), Escape close
 * with focus landing in the panel, and the cancel action for live runs
 * (consuming the previously-unused `cancelable` flag + cancelRun IPC).
 */
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { injectIpcCommands } from "../../lib/ipc/client";
import { useToastStore } from "../../lib/store/toastStore";
import { tdState, testDoubleCommands } from "../../lib/ipc/test-double";
import { useUiStore } from "../../lib/store/uiStore";
import { renderWithProviders } from "../../test/helpers";
import { RunDrawer } from "./RunDrawer";

type Task = (typeof tdState.tasks)[number];
type Run = (typeof tdState.runs)[number];

function seedTask(): void {
  tdState.tasks.push({
    id: "t-dr",
    sessionId: null,
    title: "重构内核",
    description: "",
    status: "backlog",
    createdAt: 1,
    updatedAt: 1,
    workspaceId: "__migrated__",
    workspaceLabel: null,
  });
}

function seedRun(overrides: Partial<Run>): void {
  tdState.runs.push({
    id: "r-1",
    taskId: "t-dr",
    sessionId: "s1",
    status: "succeeded",
    heartbeatAt: 1,
    kind: "single",
    cancelable: false,
    ...overrides,
  });
}

function openDrawer(): void {
  useUiStore.setState({ runDrawerTaskId: "t-dr" });
}

beforeEach(() => {
  useUiStore.setState({ runDrawerTaskId: null });
});

afterEach(() => {
  useUiStore.setState({ runDrawerTaskId: null });
});

describe("RunDrawer — header and lifecycle", () => {
  it("shows the task title instead of the raw task id", async () => {
    seedTask();
    seedRun({});
    injectIpcCommands(testDoubleCommands());

    openDrawer();
    renderWithProviders(<RunDrawer tasks={tdState.tasks as Task[]} />);

    expect(await screen.findByText(/运行记录：重构内核/)).toBeInTheDocument();
    expect(screen.queryByText(/运行记录：t-dr/)).not.toBeInTheDocument();
  });

  it("falls back to the task id when the task is no longer listed", async () => {
    seedRun({});
    injectIpcCommands(testDoubleCommands());

    openDrawer();
    renderWithProviders(<RunDrawer tasks={[]} />);

    expect(await screen.findByText(/运行记录：t-dr/)).toBeInTheDocument();
  });

  it("closes on Escape and moves focus into the panel on open", async () => {
    seedTask();
    seedRun({});
    injectIpcCommands(testDoubleCommands());

    openDrawer();
    renderWithProviders(<RunDrawer tasks={tdState.tasks as Task[]} />);

    const dialog = await screen.findByRole("dialog");
    await waitFor(() => expect(document.activeElement).toBe(dialog));

    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() =>
      expect(useUiStore.getState().runDrawerTaskId).toBe(null),
    );
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});

describe("RunDrawer — run rows", () => {
  it("localizes run statuses and badges team runs", async () => {
    seedTask();
    seedRun({ id: "r-team", kind: "team", status: "running", cancelable: true });
    seedRun({ id: "r-single", status: "succeeded" });
    injectIpcCommands(testDoubleCommands());

    openDrawer();
    renderWithProviders(<RunDrawer tasks={tdState.tasks as Task[]} />);

    expect(await screen.findByText("团队")).toBeInTheDocument();
    expect(screen.getByText("运行中")).toBeInTheDocument();
    expect(screen.getByText("已成功")).toBeInTheDocument();
    // Only the cancelable team run carries the cancel button.
    expect(screen.getAllByRole("button", { name: /取消运行/ }).length).toBe(1);
  });

  it("cancels a live run through cancelRun and toasts", async () => {
    seedTask();
    seedRun({ id: "r-live", status: "running", cancelable: true });
    const base = testDoubleCommands();
    const cancelRun = vi.fn(base.cancelRun);
    injectIpcCommands({ ...base, cancelRun });

    openDrawer();
    renderWithProviders(<RunDrawer tasks={tdState.tasks as Task[]} />);

    fireEvent.click(await screen.findByRole("button", { name: "取消运行 r-live" }));
    await waitFor(() => expect(cancelRun).toHaveBeenCalledWith("r-live"));
    await waitFor(() =>
      expect(
        useToastStore
          .getState()
          .toasts.some((toast) => toast.message.includes("取消请求已发送")),
      ).toBe(true),
    );
  });
});
