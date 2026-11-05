import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { SandboxTaskDto } from "../../../lib/ipc/bindings.gen";
import { tdState } from "../../../lib/ipc/test-double";
import { renderWithProviders } from "../../../test/helpers";
import { Toaster } from "../../../components/ui/Toaster";
import { RunStatusBar } from "./RunStatusBar";

function makeTask(id: string, status: string, taskType = "run_code"): SandboxTaskDto {
  return {
    id,
    taskType,
    runtime: "python",
    status,
    source: "print('hi')",
    submitter: "workbench",
    authorizedDirs: [],
    limits: { timeoutSecs: 10, cpuPercent: null, memoryMb: null, networkEnabled: false },
    stdout: "",
    stderr: "",
    stdoutRef: null,
    stderrRef: null,
    stdoutTruncated: false,
    stderrTruncated: false,
    artifactRef: null,
    artifactRetryable: false,
    exitInfo: status === "failed" ? "1" : null,
    durationMs: 230,
    usage: null,
    createdAt: Date.now(),
    finishedAt: Date.now(),
  };
}

describe("RunStatusBar", () => {
  it("shows nothing when there are no tasks", () => {
    tdState.sandboxTasks = [];
    renderWithProviders(
      <>
        <RunStatusBar />
        <Toaster />
      </>,
    );
    expect(screen.queryByTestId("run-status-bar")).toBeNull();
  });

  it("shows running status for a running task", async () => {
    tdState.sandboxTasks = [makeTask("task-1", "running")];
    renderWithProviders(
      <>
        <RunStatusBar />
        <Toaster />
      </>,
    );
    await waitFor(() => expect(screen.getByTestId("run-status-bar")).toBeDefined());
    expect(screen.getByText(/运行中/)).toBeDefined();
  });

  it("shows failed status with exit code for a failed task", async () => {
    tdState.sandboxTasks = [makeTask("task-err", "failed")];
    renderWithProviders(
      <>
        <RunStatusBar />
        <Toaster />
      </>,
    );
    await waitFor(() => expect(screen.getByTestId("run-status-bar")).toBeDefined());
    expect(screen.getByText(/失败/)).toBeDefined();
    expect(screen.getByText(/退出码 1/)).toBeDefined();
  });

  it("shows open terminal button", async () => {
    tdState.sandboxTasks = [makeTask("task-1", "running")];
    renderWithProviders(
      <>
        <RunStatusBar />
        <Toaster />
      </>,
    );
    await waitFor(() => expect(screen.getByTestId("run-status-bar")).toBeDefined());
    expect(screen.getByRole("button", { name: /查看终端/ })).toBeDefined();
  });
});
