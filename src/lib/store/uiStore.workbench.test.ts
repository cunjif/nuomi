/**
 * uiStore workbench-specific tests: activeArea semantics, openFile triggers
 * workbench, setView releases workbench (spec 5.3 — workbench navigation
 * state). Per-ADR-0017 editor state is bucketed by workspace id.
 */
import { beforeEach, describe, expect, it } from "vitest";
import { stubLocalStorage } from "../../test/stubStorage";
import { useUiStore } from "./uiStore";

beforeEach(() => {
  stubLocalStorage();
  useUiStore.setState({
    view: "chat",
    activeArea: "chat",
    theme: "chalkboard-dark",
    selectedSessionId: null,
    editorByWorkspace: {},
    activeWorkspaceId: null,
  });
});

describe("uiStore workbench — activeArea semantics", () => {
  it("defaults to chat area", () => {
    expect(useUiStore.getState().activeArea).toBe("chat");
  });

  it("setActiveArea toggles chat ↔ workbench", () => {
    useUiStore.getState().setActiveArea("workbench");
    expect(useUiStore.getState().activeArea).toBe("workbench");
    useUiStore.getState().setActiveArea("chat");
    expect(useUiStore.getState().activeArea).toBe("chat");
  });
});

describe("uiStore workbench — openFile triggers workbench", () => {
  it("openFile sets activeArea=workbench", () => {
    useUiStore.getState().openFile("src/main.rs", "ws-a");
    expect(useUiStore.getState().activeArea).toBe("workbench");
    expect(useUiStore.getState().editorByWorkspace["ws-a"]?.activeFile).toBe("src/main.rs");
  });

  it("openFile appends to the workspace bucket if not already open", () => {
    useUiStore.getState().openFile("a.ts", "ws-a");
    useUiStore.getState().openFile("b.ts", "ws-a");
    expect(useUiStore.getState().editorByWorkspace["ws-a"]?.openFiles).toEqual(["a.ts", "b.ts"]);
    expect(useUiStore.getState().editorByWorkspace["ws-a"]?.activeFile).toBe("b.ts");
  });

  it("openFile does not duplicate an already-open file", () => {
    useUiStore.getState().openFile("a.ts", "ws-a");
    useUiStore.getState().openFile("a.ts", "ws-a");
    expect(useUiStore.getState().editorByWorkspace["ws-a"]?.openFiles).toEqual(["a.ts"]);
  });

  it("openFile in different workspaces are independent buckets", () => {
    useUiStore.getState().openFile("a.ts", "ws-a");
    useUiStore.getState().openFile("c.ts", "ws-b");
    expect(useUiStore.getState().editorByWorkspace["ws-a"]?.openFiles).toEqual(["a.ts"]);
    expect(useUiStore.getState().editorByWorkspace["ws-b"]?.openFiles).toEqual(["c.ts"]);
  });
});

describe("uiStore workbench — setView releases workbench", () => {
  it("setView flips activeArea back to chat so the view renders", () => {
    useUiStore.setState({ activeArea: "workbench" });
    useUiStore.getState().setView("plugins");
    expect(useUiStore.getState().view).toBe("plugins");
    expect(useUiStore.getState().activeArea).toBe("chat");
  });
});

describe("uiStore workbench — selectSession returns to chat", () => {
  it("selectSession sets activeArea=chat and stores the session id", () => {
    useUiStore.setState({ activeArea: "workbench", selectedSessionId: null });
    useUiStore.getState().selectSession("sess-42");
    expect(useUiStore.getState().activeArea).toBe("chat");
    expect(useUiStore.getState().selectedSessionId).toBe("sess-42");
  });
});

describe("uiStore workbench — switchWorkspace preserves per-workspace buckets", () => {
  it("switching only flips activeWorkspaceId; each bucket is independently retained", () => {
    useUiStore.getState().openFile("src/a.ts", "ws-a");
    useUiStore.getState().openFile("src/b.ts", "ws-a");
    useUiStore.getState().openFile("lib/c.ts", "ws-b");
    useUiStore.setState({ activeWorkspaceId: "ws-a" });

    useUiStore.getState().switchWorkspace("ws-b");

    // ws-a's bucket is untouched (preserved in-place — the bucket IS the state).
    expect(useUiStore.getState().editorByWorkspace["ws-a"]?.openFiles).toEqual([
      "src/a.ts",
      "src/b.ts",
    ]);
    // ws-b's bucket is also intact.
    expect(useUiStore.getState().editorByWorkspace["ws-b"]?.openFiles).toEqual(["lib/c.ts"]);
    expect(useUiStore.getState().activeWorkspaceId).toBe("ws-b");
  });

  it("setActiveWorkspaceId updates the active workspace id", () => {
    useUiStore.setState({ activeWorkspaceId: null });
    useUiStore.getState().setActiveWorkspaceId("ws-42");
    expect(useUiStore.getState().activeWorkspaceId).toBe("ws-42");
  });
});
