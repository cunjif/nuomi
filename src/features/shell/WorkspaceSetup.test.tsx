import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Toaster } from "../../components/ui/Toaster";
import { tdState } from "../../lib/ipc/test-double";
import { injectIpcCommands } from "../../lib/ipc/client";
import { renderWithProviders } from "../../test/helpers";
import { WorkspaceSetup } from "./WorkspaceSetup";

function renderSetup(): void {
  renderWithProviders(
    <>
      <WorkspaceSetup />
      <Toaster />
    </>,
  );
}

describe("WorkspaceSetup — first-launch gating", () => {
  it("renders the path form prefilled with the current root when unconfigured", async () => {
    tdState.workspaceConfigured = false;
    renderSetup();

    expect(await screen.findByRole("heading", { name: "设置工作区" })).toBeInTheDocument();
    const input = screen.getByLabelText("工作区目录");
    // Prefill arrives via an effect once the workspace query resolves; under
    // full-suite load that can land after the first render tick.
    await waitFor(() => expect(input).toHaveValue("C:\\workspace"));
  });

  it("calls setWorkspace on submit and the mutation succeeds", async () => {
    tdState.workspaceConfigured = false;
    const setWorkspace = vi.fn(async (path: string) => {
      tdState.workspaceRoot = path;
      tdState.workspaceConfigured = true;
      return { status: "ok" as const, data: { root: path, configured: true } };
    });
    injectIpcCommands({ setWorkspace });
    renderSetup();

    fireEvent.change(await screen.findByLabelText("工作区目录"), { target: { value: "D:\\ws" } });
    fireEvent.click(screen.getByRole("button", { name: "确认" }));

    await waitFor(() => expect(setWorkspace).toHaveBeenCalledWith("D:\\ws"));
    expect(tdState.workspaceConfigured).toBe(true);
  });

  it("shows an error toast when setWorkspace fails", async () => {
    tdState.workspaceConfigured = false;
    injectIpcCommands({
      setWorkspace: vi.fn(async () => {
        throw Object.assign(new Error("boom"), { code: "workspace_invalid_path" });
      }),
    });
    renderSetup();

    const input = await screen.findByLabelText("工作区目录");
    // The form renders while the workspace query is still loading; give the
    // initial-root sync effect a tick before submitting.
    await waitFor(() => expect(input).toHaveValue("C:\\workspace"));
    fireEvent.click(screen.getByRole("button", { name: "确认" }));

    expect(await screen.findByText("设置工作区失败: 无效的文件路径")).toBeInTheDocument();
  });
});
