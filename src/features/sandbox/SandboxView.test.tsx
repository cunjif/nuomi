import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { SandboxTaskDto } from "../../lib/ipc/bindings.gen";
import { tdState } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { SandboxView } from "./SandboxView";

function seedTask(partial: Partial<SandboxTaskDto>): SandboxTaskDto {
  const now = 1700000000000;
  return {
    id: "sbx-seed",
    taskType: "run_code",
    runtime: "python",
    status: "succeeded",
    source: "print('hi')",
    authorizedDirs: [],
    limits: { timeoutSecs: 120, cpuPercent: null, memoryMb: null, networkEnabled: false },
    stdout: "",
    stderr: "",
    artifactRef: null,
    artifactRetryable: false,
    exitInfo: "exit 0",
    durationMs: 12,
    usage: null,
    createdAt: now,
    finishedAt: now + 12,
    ...partial,
  };
}

describe("SandboxView — read-only audit board (ADR-0025 Slice 1a)", () => {
  it("renders existing tasks and shows the selected task's detail", async () => {
    tdState.sandboxTasks.push(seedTask({ id: "sbx-1", stdout: "hello from sandbox" }));

    renderWithProviders(<SandboxView />);

    fireEvent.click(await screen.findByText("sbx-1"));
    // Status badge renders in both the list row and the detail header.
    expect((await screen.findAllByText("已成功")).length).toBe(2);
    expect(screen.getByLabelText("stdout")).toHaveTextContent("hello from sandbox");
    expect(screen.getByText("exit 0")).toBeInTheDocument();
    expect(screen.getByText("耗时 12 ms")).toBeInTheDocument();
  });

  it("shows executor-reported usage in the detail header", async () => {
    tdState.sandboxTasks.push(
      seedTask({ id: "sbx-usage", usage: { cpuTimeMs: 120, peakMemoryMb: 64 } }),
    );

    renderWithProviders(<SandboxView />);

    fireEvent.click(await screen.findByText("sbx-usage"));
    expect(await screen.findByText("CPU 120 ms")).toBeInTheDocument();
    expect(screen.getByText("峰值内存 64 MB")).toBeInTheDocument();
  });

  it("cancels a running task", async () => {
    tdState.sandboxTasks.push(seedTask({ id: "sbx-run", status: "running" }));

    renderWithProviders(<SandboxView />);

    fireEvent.click(await screen.findByText("sbx-run"));
    fireEvent.click(await screen.findByRole("button", { name: "取消任务" }));

    await waitFor(() => expect(screen.getByText("已取消")).toBeInTheDocument());
  });

  it("retries a failed artifact transfer", async () => {
    tdState.sandboxTasks.push(seedTask({ id: "sbx-art", artifactRetryable: true }));

    renderWithProviders(<SandboxView />);

    fireEvent.click(await screen.findByText("sbx-art"));
    fireEvent.click(await screen.findByRole("button", { name: "重试回传" }));

    await waitFor(() =>
      expect(screen.getByText("C:\\sandbox\\sbx-art\\out.zip")).toBeInTheDocument(),
    );
  });

  it("shows the unavailable hint when the sandbox plugin reports no runtimes", async () => {
    tdState.sandboxRuntimes.length = 0;

    renderWithProviders(<SandboxView />);

    expect(await screen.findByText("沙箱服务不可用")).toBeInTheDocument();
  });
});
