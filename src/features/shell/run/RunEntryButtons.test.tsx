import { screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderWithProviders } from "../../../test/helpers";
import { Toaster } from "../../../components/ui/Toaster";
import { useUiStore } from "../../../lib/store/uiStore";
import { tdSeedFiles } from "../../../lib/ipc/test-double";
import { RunEntryButtons } from "./RunEntryButtons";

function setActiveFile(path: string): void {
  useUiStore.setState({
    activeWorkspaceId: "ws-test",
    editorByWorkspace: {
      "ws-test": {
        openFiles: [path],
        activeFile: path,
        selectedPaths: [],
        lastSelectedPath: null,
        clipboardPaths: [],
        clipboardMode: null,
        dirtyPaths: {},
        crossRefs: {},
      },
    },
  });
}

describe("RunEntryButtons", () => {
  it("renders clickable run/build buttons when runtimes are available", async () => {
    tdSeedFiles({ "main.py": "print('hi')" });
    setActiveFile("main.py");
    renderWithProviders(
      <>
        <RunEntryButtons />
        <Toaster />
      </>,
    );

    const runBtn = await screen.findByRole("button", { name: "运行代码" });
    const buildBtn = screen.getByRole("button", { name: "构建应用" });
    await waitFor(() => expect(runBtn).not.toBeDisabled());
    expect(buildBtn).not.toBeDisabled();
    expect(screen.queryByTestId("sandbox-unreachable-hint")).toBeNull();
  });

  it("disables buttons and shows hint when runtimes list is empty", async () => {
    useUiStore.setState({ activeWorkspaceId: null, editorByWorkspace: {} });
    // Override the test-double to return an empty runtime list.
    const { injectIpcCommands } = await import("../../../lib/ipc/client");
    const { testDoubleCommands } = await import("../../../lib/ipc/test-double");
    const base = testDoubleCommands();
    injectIpcCommands({ ...base, sandboxListRuntimes: async () => ({ status: "ok", data: [] as never[] }) });

    renderWithProviders(
      <>
        <RunEntryButtons />
        <Toaster />
      </>,
    );

    const runBtn = await screen.findByRole("button", { name: "运行代码" });
    await waitFor(() => expect(runBtn).toBeDisabled());
    expect(screen.getByRole("button", { name: "构建应用" })).toBeDisabled();
  });

  it("disables buttons when the sandbox probe times out", async () => {
    useUiStore.setState({ activeWorkspaceId: null, editorByWorkspace: {} });
    const { injectIpcCommands } = await import("../../../lib/ipc/client");
    const { testDoubleCommands } = await import("../../../lib/ipc/test-double");
    const base = testDoubleCommands();
    // Never resolves → probe times out.
    injectIpcCommands({
      ...base,
      sandboxListRuntimes: () => new Promise(() => undefined),
    });

    renderWithProviders(
      <>
        <RunEntryButtons />
        <Toaster />
      </>,
    );

    const runBtn = await screen.findByRole("button", { name: "运行代码" });
    expect(runBtn).toBeDisabled();
    await waitFor(
      () => expect(screen.getByTestId("sandbox-unreachable-hint")).toBeDefined(),
      { timeout: 4000 },
    );
  });
});
