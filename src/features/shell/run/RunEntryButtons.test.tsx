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
  it("renders clickable run/build buttons when a runnable file is active", async () => {
    tdSeedFiles({ "main.py": "print('hi')" });
    setActiveFile("main.py");
    renderWithProviders(
      <>
        <RunEntryButtons />
        <Toaster />
      </>,
    );

    const runBtn = await screen.findByRole("button", { name: "运行代码" });
    await waitFor(() => expect(runBtn).not.toBeDisabled());
  });

  it("disables run button when no active file", async () => {
    useUiStore.setState({ activeWorkspaceId: null, editorByWorkspace: {} });
    renderWithProviders(
      <>
        <RunEntryButtons />
        <Toaster />
      </>,
    );

    const runBtn = await screen.findByRole("button", { name: "运行代码" });
    expect(runBtn).toBeDisabled();
  });

  it("disables run button for unsupported languages", async () => {
    tdSeedFiles({ "README.md": "# title" });
    setActiveFile("README.md");
    renderWithProviders(
      <>
        <RunEntryButtons />
        <Toaster />
      </>,
    );

    const runBtn = await screen.findByRole("button", { name: "运行代码" });
    await waitFor(() => expect(runBtn).toBeDisabled());
  });
});
