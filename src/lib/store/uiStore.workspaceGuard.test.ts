/**
 * uiStore unsaved-edits guard tests (workspace-level switch/close): the
 * guard parks the action in `pendingWorkspaceAction` when the editor bucket
 * carries dirty files, executes it directly otherwise, and confirm drops
 * the dirty marks before executing. IPC rides the test-double (in-memory
 * workspace state).
 */
import { waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
// Side-effect import: injects the IPC test-double (workspaceSwitch/Close
// ride it) and resets the in-memory state before each test.
import "../../test/helpers";
import { stubLocalStorage } from "../../test/stubStorage";
import { tdState } from "../../lib/ipc/test-double-state";
import { useUiStore } from "./uiStore";

function seedWorkspace(id: string): void {
  tdState.workspaces.push({
    id,
    rootPath: `C:\\root-${id}`,
    colorTag: "ink-blue",
    createdAt: 1,
    isActive: false,
    isFocused: false,
    isOpen: false,
    directoryPresent: true,
    label: id,
    labelFull: id,
    directories: [],
  });
}

function markDirty(workspaceId: string, path: string): void {
  useUiStore.getState().openFile(path, workspaceId);
  useUiStore.getState().markDirty(path, true, workspaceId);
}

beforeEach(() => {
  stubLocalStorage();
  tdState.workspaces.length = 0;
  seedWorkspace("ws-a");
  seedWorkspace("ws-b");
  tdState.workspaces.find((w) => w.id === "ws-a")!.isActive = true;
  useUiStore.setState({
    activeWorkspaceId: "ws-a",
    editorByWorkspace: {},
    pendingWorkspaceAction: null,
  });
});

describe("dirtyFilesOf", () => {
  it("returns only entries marked true", () => {
    markDirty("ws-a", "a.ts");
    useUiStore.getState().openFile("b.ts", "ws-a");
    expect(useUiStore.getState().dirtyFilesOf("ws-a")).toEqual(["a.ts"]);
  });

  it("returns empty for a bucket-less workspace", () => {
    expect(useUiStore.getState().dirtyFilesOf("ws-none")).toEqual([]);
  });
});

describe("requestSwitchToWorkspace — guard", () => {
  it("switches directly when the active bucket is clean", async () => {
    const executed = useUiStore.getState().requestSwitchToWorkspace("ws-b");
    expect(executed).toBe(true);
    // Direct switches are fire-and-forget async — the mirror lands shortly.
    await waitFor(() => {
      expect(useUiStore.getState().activeWorkspaceId).toBe("ws-b");
    });
    expect(useUiStore.getState().pendingWorkspaceAction).toBeNull();
  });

  it("parks the action when the active bucket is dirty", () => {
    markDirty("ws-a", "draft.md");
    const executed = useUiStore.getState().requestSwitchToWorkspace("ws-b");
    expect(executed).toBe(false);
    expect(useUiStore.getState().activeWorkspaceId).toBe("ws-a");
    const pending = useUiStore.getState().pendingWorkspaceAction;
    expect(pending?.kind).toBe("switch");
    expect(pending?.workspaceId).toBe("ws-a");
    expect(pending?.targetWorkspaceId).toBe("ws-b");
    expect(pending?.dirtyFiles).toEqual(["draft.md"]);
  });

  it("never guards a no-op switch to the active workspace", () => {
    markDirty("ws-a", "draft.md");
    const executed = useUiStore.getState().requestSwitchToWorkspace("ws-a");
    expect(executed).toBe(true);
    expect(useUiStore.getState().pendingWorkspaceAction).toBeNull();
  });
});

describe("requestCloseWorkspace — guard", () => {
  it("closes directly when the bucket is clean", async () => {
    const executed = await useUiStore.getState().requestCloseWorkspace("ws-a");
    expect(executed).toBe(true);
    expect(useUiStore.getState().pendingWorkspaceAction).toBeNull();
    // closeWorkspaceViaIpc re-syncs the mirror; test-double marks ws-a closed.
    expect(tdState.workspaces.find((w) => w.id === "ws-a")?.isActive).toBe(false);
  });

  it("parks the action when the bucket is dirty", async () => {
    markDirty("ws-a", "draft.md");
    const executed = await useUiStore.getState().requestCloseWorkspace("ws-a");
    expect(executed).toBe(false);
    const pending = useUiStore.getState().pendingWorkspaceAction;
    expect(pending?.kind).toBe("close");
    expect(pending?.workspaceId).toBe("ws-a");
    expect(pending?.dirtyFiles).toEqual(["draft.md"]);
  });
});

describe("confirmPendingWorkspaceAction", () => {
  it("drops dirty marks and executes the parked switch", async () => {
    markDirty("ws-a", "draft.md");
    useUiStore.getState().requestSwitchToWorkspace("ws-b");
    await useUiStore.getState().confirmPendingWorkspaceAction();
    expect(useUiStore.getState().activeWorkspaceId).toBe("ws-b");
    expect(useUiStore.getState().pendingWorkspaceAction).toBeNull();
    expect(useUiStore.getState().dirtyFilesOf("ws-a")).toEqual([]);
  });

  it("drops dirty marks and executes the parked close", async () => {
    markDirty("ws-a", "draft.md");
    await useUiStore.getState().requestCloseWorkspace("ws-a");
    await useUiStore.getState().confirmPendingWorkspaceAction();
    expect(useUiStore.getState().pendingWorkspaceAction).toBeNull();
    expect(useUiStore.getState().dirtyFilesOf("ws-a")).toEqual([]);
    expect(tdState.workspaces.find((w) => w.id === "ws-a")?.isActive).toBe(false);
  });

  it("is a no-op without a pending action", async () => {
    await useUiStore.getState().confirmPendingWorkspaceAction();
    expect(useUiStore.getState().activeWorkspaceId).toBe("ws-a");
  });
});

describe("cancelPendingWorkspaceAction", () => {
  it("keeps the dirty marks and the active workspace", () => {
    markDirty("ws-a", "draft.md");
    useUiStore.getState().requestSwitchToWorkspace("ws-b");
    useUiStore.getState().cancelPendingWorkspaceAction();
    expect(useUiStore.getState().pendingWorkspaceAction).toBeNull();
    expect(useUiStore.getState().activeWorkspaceId).toBe("ws-a");
    expect(useUiStore.getState().dirtyFilesOf("ws-a")).toEqual(["draft.md"]);
  });
});

describe("requestCloseWorkspace — running-task probe", () => {
  function seedRunningRun(runId: string, title: string): void {
    tdState.tasks.push({
      id: `task-${runId}`,
      sessionId: null,
      title,
      description: "",
      status: "running",
      createdAt: 1,
      updatedAt: 1,
      workspaceId: "ws-a",
      workspaceLabel: null,
    });
    tdState.runs.push({
      id: runId,
      taskId: `task-${runId}`,
      sessionId: "sess",
      status: "running",
      heartbeatAt: 1,
      kind: "single",
      cancelable: true,
    });
  }

  it("parks the close when the workspace has active runs (clean bucket)", async () => {
    seedRunningRun("r-1", "deploy site");
    const executed = await useUiStore.getState().requestCloseWorkspace("ws-a");
    expect(executed).toBe(false);
    const pending = useUiStore.getState().pendingWorkspaceAction;
    expect(pending?.kind).toBe("close");
    expect(pending?.dirtyFiles).toEqual([]);
    expect(pending?.runningTasks).toHaveLength(1);
    expect(pending?.runningTasks[0]?.taskTitle).toBe("deploy site");
    // Still open — the guard did not close.
    expect(tdState.workspaces.find((w) => w.id === "ws-a")?.isActive).toBe(true);
  });

  it("confirm force-closes past the backend refusal", async () => {
    seedRunningRun("r-2", "run tests");
    await useUiStore.getState().requestCloseWorkspace("ws-a");
    await useUiStore.getState().confirmPendingWorkspaceAction();
    expect(useUiStore.getState().pendingWorkspaceAction).toBeNull();
    // force=true skips the close_needs_confirm refusal in the double.
    expect(tdState.workspaces.find((w) => w.id === "ws-a")?.isActive).toBe(false);
  });

  it("a clean workspace with no runs closes without parking", async () => {
    const executed = await useUiStore.getState().requestCloseWorkspace("ws-a");
    expect(executed).toBe(true);
    await waitFor(() => {
      expect(tdState.workspaces.find((w) => w.id === "ws-a")?.isActive).toBe(false);
    });
    expect(useUiStore.getState().pendingWorkspaceAction).toBeNull();
  });
});
