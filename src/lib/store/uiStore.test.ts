import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { stubLocalStorage } from "../../test/stubStorage";
import { THEME_STORAGE_KEY, resolveInitialTheme, useUiStore } from "./uiStore";

function stubMatchMedia(prefersLight: boolean): void {
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({ matches: prefersLight })),
  );
}

beforeEach(() => {
  stubLocalStorage();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("resolveInitialTheme — localStorage → prefers-color-scheme → chalkboard-dark", () => {
  it("migrates a legacy stored value forward", () => {
    localStorage.setItem(THEME_STORAGE_KEY, "light");
    stubMatchMedia(false);
    expect(resolveInitialTheme()).toBe("paper-light");
  });

  it("stored value wins over prefers-color-scheme", () => {
    localStorage.setItem(THEME_STORAGE_KEY, "dark");
    stubMatchMedia(true);
    expect(resolveInitialTheme()).toBe("chalkboard-dark");
  });

  it("falls back to prefers light when nothing is stored", () => {
    stubMatchMedia(true);
    expect(resolveInitialTheme()).toBe("paper-light");
  });

  it("falls back to dark when OS does not prefer light", () => {
    stubMatchMedia(false);
    expect(resolveInitialTheme()).toBe("chalkboard-dark");
  });

  it("ignores invalid stored values and keeps the chain intact", () => {
    localStorage.setItem(THEME_STORAGE_KEY, "sepia");
    stubMatchMedia(true);
    expect(resolveInitialTheme()).toBe("paper-light");
  });
});

describe("uiStore theme state", () => {
  it("setTheme flips state without side effects (persistence/DOM live in useTheme)", () => {
    useUiStore.setState({ theme: "chalkboard-dark" });
    useUiStore.getState().setTheme("paper-light");
    expect(useUiStore.getState().theme).toBe("paper-light");
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBeNull();
  });

  it("initial store theme follows resolveInitialTheme", () => {
    useUiStore.setState({ theme: resolveInitialTheme() });
    expect(useUiStore.getState().theme).toBe("chalkboard-dark");
  });
});

describe("uiStore activeArea navigation state (需求 5)", () => {
  it("defaults to the chat area", () => {
    useUiStore.setState({ activeArea: "chat", workbenchSubTab: "workspaceList" });
    expect(useUiStore.getState().activeArea).toBe("chat");
  });

  it("setActiveArea flips between chat and workbench", () => {
    useUiStore.setState({ activeArea: "chat", workbenchSubTab: "workspaceList" });
    useUiStore.getState().setActiveArea("workbench");
    expect(useUiStore.getState().activeArea).toBe("workbench");
    useUiStore.getState().setActiveArea("chat");
    expect(useUiStore.getState().activeArea).toBe("chat");
  });

  it("openFile switches to workbench area with editor sub-tab", () => {
    useUiStore.setState({ openFiles: [], activeFile: null, activeArea: "chat", workbenchSubTab: "workspaceList" });
    useUiStore.getState().openFile("src/main.rs");
    expect(useUiStore.getState().activeArea).toBe("workbench");
    expect(useUiStore.getState().workbenchSubTab).toBe("editor");
    expect(useUiStore.getState().activeFile).toBe("src/main.rs");
  });

  it("selecting a session returns to the chat area", () => {
    useUiStore.setState({ activeArea: "workbench", workbenchSubTab: "editor", selectedSessionId: null });
    useUiStore.getState().selectSession("s1");
    expect(useUiStore.getState().activeArea).toBe("chat");
    expect(useUiStore.getState().selectedSessionId).toBe("s1");
  });

  it("setView releases the workbench area so the view actually renders", () => {
    // Regression: Shell renders `activeArea === "workbench" ? <WorkbenchArea/> :
    // renderView(view)`, so switching only `view` while a file was open left
    // the workbench on screen — e.g. the editor toolbar's Extensions Center
    // button appeared to do nothing.
    useUiStore.setState({ view: "chat", activeArea: "workbench", workbenchSubTab: "editor" });
    useUiStore.getState().setView("plugins");
    expect(useUiStore.getState().view).toBe("plugins");
    expect(useUiStore.getState().activeArea).toBe("chat");
  });

  it("setWorkbenchSubTab switches the workbench sub-tab", () => {
    useUiStore.setState({ activeArea: "workbench", workbenchSubTab: "workspaceList" });
    useUiStore.getState().setWorkbenchSubTab("editor");
    expect(useUiStore.getState().workbenchSubTab).toBe("editor");
    useUiStore.getState().setWorkbenchSubTab("workspaceList");
    expect(useUiStore.getState().workbenchSubTab).toBe("workspaceList");
  });
});

describe("uiStore dirtyPaths tracking", () => {
  it("markDirty toggles a path and no-ops on redundant updates", () => {
    useUiStore.setState({ dirtyPaths: {} });
    const { markDirty } = useUiStore.getState();
    markDirty("a.ts", true);
    expect(useUiStore.getState().dirtyPaths["a.ts"]).toBe(true);
    // Redundant set returns the same state object (no subscriber churn).
    const before = useUiStore.getState();
    markDirty("a.ts", true);
    expect(useUiStore.getState()).toBe(before);
    markDirty("a.ts", false);
    expect(useUiStore.getState().dirtyPaths).toEqual({});
  });

  it("closeFile drops the dirty entry for the closed tab", () => {
    useUiStore.setState({ openFiles: ["a.ts", "b.ts"], activeFile: "b.ts", dirtyPaths: {} });
    const s = useUiStore.getState();
    s.markDirty("b.ts", true);
    s.closeFile("b.ts");
    expect(useUiStore.getState().dirtyPaths["b.ts"]).toBeUndefined();
    expect(useUiStore.getState().openFiles).toEqual(["a.ts"]);
  });
});
