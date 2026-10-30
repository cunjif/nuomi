import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { SandboxTaskDto } from "../../../lib/ipc/bindings.gen";
import { tdState } from "../../../lib/ipc/test-double";
import { renderWithProviders } from "../../../test/helpers";
import { Toaster } from "../../../components/ui/Toaster";
import { RunPanelOverlay } from "./RunPanelOverlay";

function makeTask(id: string, status: string, stdout = ""): SandboxTaskDto {
  return {
    id,
    taskType: "run_code",
    runtime: "python",
    status,
    source: "print('hi')",
    authorizedDirs: [],
    limits: { timeoutSecs: 10, cpuPercent: null, memoryMb: null, networkEnabled: false },
    stdout,
    stderr: "",
    artifactRef: null,
    artifactRetryable: false,
    exitInfo: null,
    durationMs: 100,
    usage: null,
    createdAt: Date.now(),
    finishedAt: Date.now(),
  };
}

describe("RunPanelOverlay", () => {
  it("renders a single task with status and stdout", async () => {
    tdState.sandboxTasks = [makeTask("task-1", "succeeded", "hello\n")];
    renderWithProviders(
      <>
        <RunPanelOverlay workspaceId="ws-test" />
        <Toaster />
      </>,
    );

    await screen.findByTestId("run-panel-overlay");
    expect(await screen.findByText("已成功")).toBeDefined();
    expect(await screen.findByText("hello")).toBeDefined();
  });

  it("switches between multiple task tabs", async () => {
    tdState.sandboxTasks = [
      makeTask("task-a", "succeeded", "out-a"),
      makeTask("task-b", "running", "out-b"),
    ];
    renderWithProviders(
      <>
        <RunPanelOverlay workspaceId="ws-test" />
        <Toaster />
      </>,
    );

    await screen.findByTestId("run-panel-overlay");
    expect(await screen.findByText("out-a")).toBeDefined();
    const tabB = await screen.findByRole("button", { name: /task-b/ });
    tabB.click();
    await waitFor(() => expect(screen.getByText("out-b")).toBeDefined());
  });

  it("collapses and expands without blocking the editor", async () => {
    tdState.sandboxTasks = [makeTask("task-1", "succeeded", "hello")];
    renderWithProviders(
      <>
        <RunPanelOverlay workspaceId="ws-test" />
        <Toaster />
      </>,
    );

    await screen.findByTestId("run-panel-overlay");
    const collapseBtn = screen.getByRole("button", { name: "折叠" });
    collapseBtn.click();
    await waitFor(() => expect(screen.getByTestId("run-panel-collapsed")).toBeDefined());
    const expandBtn = screen.getByRole("button", { name: "展开" });
    expandBtn.click();
    await waitFor(() => expect(screen.getByTestId("run-panel-overlay")).toBeDefined());
  });

  it("shows error state for a failed task with retry affordance", async () => {
    tdState.sandboxTasks = [makeTask("task-err", "failed", "")];
    renderWithProviders(
      <>
        <RunPanelOverlay workspaceId="ws-test" />
        <Toaster />
      </>,
    );

    await screen.findByTestId("run-panel-overlay");
    expect(await screen.findByText("已失败")).toBeDefined();
  });
});
