/**
 * WorkspaceBar guard-wiring tests: the tab switch and the × close button
 * both go through the uiStore guarded requests — a dirty editor bucket
 * parks the action (dialog shows) instead of silently discarding edits.
 */
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { renderWithProviders } from "../../test/helpers";
import { tdState } from "../../lib/ipc/test-double-state";
import { useUiStore } from "../../lib/store/uiStore";
import { WorkspaceGuardDialog } from "./WorkspaceGuardDialog";
import { WorkspaceBar } from "./WorkspaceBar";

function seedWorkspace(id: string, label: string, active: boolean): void {
  tdState.workspaces.push({
    id,
    rootPath: `C:\\root-${id}`,
    colorTag: "ink-blue",
    createdAt: 1,
    isActive: active,
    isFocused: active,
    isOpen: active,
    directoryPresent: true,
    label,
    labelFull: label,
    directories: [],
  });
}

function renderBar(): ReturnType<typeof renderWithProviders> {
  return renderWithProviders(
    <>
      <WorkspaceBar />
      <WorkspaceGuardDialog />
    </>,
  );
}

beforeEach(() => {
  tdState.workspaces.length = 0;
  seedWorkspace("ws-a", "proj-a", true);
  seedWorkspace("ws-b", "proj-b", false);
  useUiStore.setState({
    activeWorkspaceId: "ws-a",
    editorByWorkspace: {},
    pendingWorkspaceAction: null,
  });
});

describe("WorkspaceBar — guarded × close", () => {
  it("closes directly when the bucket is clean", async () => {
    renderBar();
    fireEvent.click(await screen.findByText("×"));
    await waitFor(() => {
      expect(useUiStore.getState().pendingWorkspaceAction).toBeNull();
    });
    expect(tdState.workspaces.find((w) => w.id === "ws-a")?.isActive).toBe(false);
  });

  it("parks the close behind the guard when the workspace bucket is dirty", async () => {
    useUiStore.getState().openFile("draft.md", "ws-a");
    useUiStore.getState().markDirty("draft.md", true, "ws-a");
    renderBar();
    // The × affordance is a nested span inside the workspace tab button.
    fireEvent.click(await screen.findByText("×"));
    // The guarded close probes asynchronously — the action parks shortly.
    await waitFor(() => {
      expect(useUiStore.getState().pendingWorkspaceAction?.kind).toBe("close");
    });
    expect(useUiStore.getState().dirtyFilesOf("ws-a")).toEqual(["draft.md"]);
    // The guard dialog (mounted alongside) shows what would be lost.
    expect(await screen.findByText("draft.md")).toBeInTheDocument();
    expect(tdState.workspaces.find((w) => w.id === "ws-a")?.isActive).toBe(true);
  });
});
