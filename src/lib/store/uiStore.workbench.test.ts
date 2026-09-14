/**
 * uiStore workbench-specific tests: activeArea semantics, openFile triggers
 * workbench+editor, setView releases workbench, and setWorkbenchSubTab
 * round-trips (spec 5.3 — workbench navigation state).
 */
import { beforeEach, describe, expect, it } from "vitest";
import { stubLocalStorage } from "../../test/stubStorage";
import { useUiStore } from "./uiStore";

beforeEach(() => {
  stubLocalStorage();
  useUiStore.setState({
    view: "chat",
    activeArea: "chat",
    workbenchSubTab: "workspaceList",
    theme: "chalkboard-dark",
    selectedSessionId: null,
    openFiles: [],
    activeFile: null,
    dirtyPaths: {},
    activeWorkspaceId: null,
    workspaceTabs: {},
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

describe("uiStore workbench — openFile triggers workbench+editor", () => {
  it("openFile sets activeArea=workbench and workbenchSubTab=editor", () => {
    useUiStore.getState().openFile("src/main.rs");
    expect(useUiStore.getState().activeArea).toBe("workbench");
    expect(useUiStore.getState().workbenchSubTab).toBe("editor");
    expect(useUiStore.getState().activeFile).toBe("src/main.rs");
  });

  it("openFile appends to openFiles if not already open", () => {
    useUiStore.setState({ openFiles: ["a.ts"], activeFile: "a.ts" });
    useUiStore.getState().openFile("b.ts");
    expect(useUiStore.getState().openFiles).toEqual(["a.ts", "b.ts"]);
    expect(useUiStore.getState().activeFile).toBe("b.ts");
  });

  it("openFile does not duplicate an already-open file", () => {
    useUiStore.setState({ openFiles: ["a.ts"], activeFile: "a.ts" });
    useUiStore.getState().openFile("a.ts");
    expect(useUiStore.getState().openFiles).toEqual(["a.ts"]);
  });
});

describe("uiStore workbench — setView releases workbench", () => {
  it("setView flips activeArea back to chat so the view renders", () => {
    useUiStore.setState({ activeArea: "workbench", workbenchSubTab: "editor" });
    useUiStore.getState().setView("plugins");
    expect(useUiStore.getState().view).toBe("plugins");
    expect(useUiStore.getState().activeArea).toBe("chat");
  });
});

describe("uiStore workbench — setWorkbenchSubTab", () => {
  it("switches between workspaceList and editor", () => {
    useUiStore.getState().setWorkbenchSubTab("editor");
    expect(useUiStore.getState().workbenchSubTab).toBe("editor");
    useUiStore.getState().setWorkbenchSubTab("workspaceList");
    expect(useUiStore.getState().workbenchSubTab).toBe("workspaceList");
  });

  it("does not affect activeArea", () => {
    useUiStore.setState({ activeArea: "workbench" });
    useUiStore.getState().setWorkbenchSubTab("editor");
    expect(useUiStore.getState().activeArea).toBe("workbench");
  });
});

describe("uiStore workbench — selectSession returns to chat", () => {
  it("selectSession sets activeArea=chat and stores the session id", () => {
    useUiStore.setState({ activeArea: "workbench", workbenchSubTab: "editor", selectedSessionId: null });
    useUiStore.getState().selectSession("sess-42");
    expect(useUiStore.getState().activeArea).toBe("chat");
    expect(useUiStore.getState().selectedSessionId).toBe("sess-42");
  });
});

describe("uiStore workbench — switchWorkspace tab save/restore", () => {
  it("saves current workspace tabs and restores target tabs on switch", () => {
    useUiStore.setState({
      activeWorkspaceId: "ws-a",
      openFiles: ["src/a.ts", "src/b.ts"],
      activeFile: "src/b.ts",
      workbenchSubTab: "editor",
      workspaceTabs: {
        "ws-b": { openFiles: ["lib/c.ts"], activeFile: "lib/c.ts" },
      },
    });

    useUiStore.getState().switchWorkspace("ws-b");

    // ws-a's tabs were saved.
    expect(useUiStore.getState().workspaceTabs["ws-a"]).toEqual({
      openFiles: ["src/a.ts", "src/b.ts"],
      activeFile: "src/b.ts",
    });
    // ws-b's tabs were restored.
    expect(useUiStore.getState().openFiles).toEqual(["lib/c.ts"]);
    expect(useUiStore.getState().activeFile).toBe("lib/c.ts");
    expect(useUiStore.getState().activeWorkspaceId).toBe("ws-b");
    // Editor sub-tab because tabs were restored.
    expect(useUiStore.getState().workbenchSubTab).toBe("editor");
  });

  it("always switches to editor sub-tab even without saved tabs", () => {
    useUiStore.setState({
      activeWorkspaceId: "ws-a",
      openFiles: ["src/a.ts"],
      activeFile: "src/a.ts",
      workbenchSubTab: "editor",
      workspaceTabs: {},
    });

    useUiStore.getState().switchWorkspace("ws-c");

    expect(useUiStore.getState().openFiles).toEqual([]);
    expect(useUiStore.getState().activeFile).toBeNull();
    expect(useUiStore.getState().workbenchSubTab).toBe("editor");
  });

  it("does not save tabs when no workspace was active", () => {
    useUiStore.setState({
      activeWorkspaceId: null,
      openFiles: ["src/x.ts"],
      activeFile: "src/x.ts",
      workspaceTabs: {},
    });

    useUiStore.getState().switchWorkspace("ws-d");

    expect(useUiStore.getState().workspaceTabs).toEqual({});
    expect(useUiStore.getState().openFiles).toEqual([]);
    expect(useUiStore.getState().activeWorkspaceId).toBe("ws-d");
  });

  it("setActiveWorkspaceId updates the active workspace id", () => {
    useUiStore.setState({ activeWorkspaceId: null });
    useUiStore.getState().setActiveWorkspaceId("ws-42");
    expect(useUiStore.getState().activeWorkspaceId).toBe("ws-42");
  });
});
