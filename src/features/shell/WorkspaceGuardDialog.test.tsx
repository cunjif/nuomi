/**
 * WorkspaceGuardDialog tests: renders only for a parked action, lists the
 * dirty files, cancel keeps everything, confirm executes the parked action,
 * and Esc cancels (Dialog base behaviour).
 */
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { tdReset } from "../../lib/ipc/test-double";
import { tdState } from "../../lib/ipc/test-double-state";
import { useUiStore } from "../../lib/store/uiStore";
import { WorkspaceGuardDialog } from "./WorkspaceGuardDialog";

function parkSwitch(dirtyFiles: string[]): void {
  useUiStore.setState({
    activeWorkspaceId: "ws-a",
    pendingWorkspaceAction: {
      kind: "switch",
      workspaceId: "ws-a",
      targetWorkspaceId: "ws-b",
      workspaceLabel: "proj-a",
      dirtyFiles,
    },
  });
}

function parkClose(dirtyFiles: string[]): void {
  useUiStore.setState({
    activeWorkspaceId: "ws-a",
    pendingWorkspaceAction: {
      kind: "close",
      workspaceId: "ws-a",
      targetWorkspaceId: "ws-a",
      workspaceLabel: "proj-a",
      dirtyFiles,
    },
  });
}

beforeEach(() => {
  tdReset();
  tdState.workspaces.push(
    {
      id: "ws-a",
      rootPath: "C:\\a",
      colorTag: "ink-blue",
      createdAt: 1,
      isActive: true,
      isFocused: true,
      isOpen: true,
      directoryPresent: true,
      label: "proj-a",
      labelFull: "proj-a",
      directories: [],
    },
    {
      id: "ws-b",
      rootPath: "C:\\b",
      colorTag: "ink-green",
      createdAt: 1,
      isActive: false,
      isFocused: false,
      isOpen: false,
      directoryPresent: true,
      label: "proj-b",
      labelFull: "proj-b",
      directories: [],
    },
  );
  useUiStore.setState({
    activeWorkspaceId: "ws-a",
    editorByWorkspace: {},
    pendingWorkspaceAction: null,
  });
});

describe("WorkspaceGuardDialog", () => {
  it("renders nothing without a pending action", () => {
    renderWithProviders(<WorkspaceGuardDialog />);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("lists the dirty files for a parked switch", () => {
    parkSwitch(["src/main.rs", "notes.md"]);
    renderWithProviders(<WorkspaceGuardDialog />);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("src/main.rs")).toBeInTheDocument();
    expect(screen.getByText("notes.md")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "放弃更改并切换" })).toBeInTheDocument();
  });

  it("shows the close wording for a parked close", () => {
    parkClose(["a.ts"]);
    renderWithProviders(<WorkspaceGuardDialog />);
    expect(screen.getByRole("button", { name: "放弃更改并关闭" })).toBeInTheDocument();
  });

  it("cancel keeps the pending state machinery to the store (dialog closes)", () => {
    parkSwitch(["a.ts"]);
    useUiStore.getState().openFile("a.ts", "ws-a");
    useUiStore.getState().markDirty("a.ts", true, "ws-a");
    renderWithProviders(<WorkspaceGuardDialog />);
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(useUiStore.getState().pendingWorkspaceAction).toBeNull();
    expect(useUiStore.getState().activeWorkspaceId).toBe("ws-a");
    expect(useUiStore.getState().dirtyFilesOf("ws-a")).toEqual(["a.ts"]);
  });

  it("confirm executes the parked switch and drops dirty marks", async () => {
    parkSwitch(["a.ts"]);
    useUiStore.getState().openFile("a.ts", "ws-a");
    useUiStore.getState().markDirty("a.ts", true, "ws-a");
    renderWithProviders(<WorkspaceGuardDialog />);
    fireEvent.click(screen.getByRole("button", { name: "放弃更改并切换" }));
    await waitFor(() => {
      expect(useUiStore.getState().activeWorkspaceId).toBe("ws-b");
    });
    expect(useUiStore.getState().dirtyFilesOf("ws-a")).toEqual([]);
    expect(useUiStore.getState().pendingWorkspaceAction).toBeNull();
  });

  it("Esc cancels (Dialog base behaviour)", () => {
    parkSwitch(["a.ts"]);
    renderWithProviders(<WorkspaceGuardDialog />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(useUiStore.getState().pendingWorkspaceAction).toBeNull();
    expect(useUiStore.getState().activeWorkspaceId).toBe("ws-a");
  });
});
