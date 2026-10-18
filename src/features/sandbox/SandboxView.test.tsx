import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SandboxTaskDto } from "../../lib/ipc/bindings.gen";
import { tdState } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { SandboxView } from "./SandboxView";

// Monaco never loads in jsdom — stand in with a plain textarea
// (mirrors MonacoTab.test.tsx).
vi.mock("@monaco-editor/react", () => ({
  default: (props: { value: string; onChange: (v: string) => void }) => (
    <textarea aria-label="editor" value={props.value} onChange={(e) => props.onChange(e.target.value)} />
  ),
}));
// Skip the self-hosted monaco wiring (imports the real editor kernel).
vi.mock("../shell/monacoSetup", () => ({}));

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
    createdAt: now,
    finishedAt: now + 12,
    ...partial,
  };
}

/** Toolbar buttons stay disabled until the runtimes query resolves. */
async function clickToolbarButton(name: string): Promise<void> {
  const btn = await screen.findByRole("button", { name });
  await waitFor(() => expect(btn).toBeEnabled());
  fireEvent.click(btn);
}

describe("SandboxView — task list + detail (task 7.7)", () => {
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

  it("submits a run task from the dialog and lists it", async () => {
    renderWithProviders(<SandboxView />);

    await clickToolbarButton("新建运行");
    fireEvent.change(await screen.findByLabelText("editor"), { target: { value: "print('ok')" } });
    fireEvent.click(screen.getByRole("button", { name: "运行" }));

    expect(await screen.findByText("运行代码 · python")).toBeInTheDocument();
    expect(await screen.findByText("已成功")).toBeInTheDocument();
  });

  it("opens the build dialog with its hint", async () => {
    renderWithProviders(<SandboxView />);

    await clickToolbarButton("构建应用");

    expect(await screen.findByText(/签名密钥保留在宿主/)).toBeInTheDocument();
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

    expect(await screen.findByText("沙箱不可用：请检查沙箱插件配置")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "新建运行" })).toBeDisabled();
  });
});
