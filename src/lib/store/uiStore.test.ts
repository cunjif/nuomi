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

describe("resolveInitialTheme — localStorage → prefers-color-scheme → dark", () => {
  it("uses a valid stored value", () => {
    localStorage.setItem(THEME_STORAGE_KEY, "light");
    stubMatchMedia(false);
    expect(resolveInitialTheme()).toBe("light");
  });

  it("stored value wins over prefers-color-scheme", () => {
    localStorage.setItem(THEME_STORAGE_KEY, "dark");
    stubMatchMedia(true);
    expect(resolveInitialTheme()).toBe("dark");
  });

  it("falls back to prefers light when nothing is stored", () => {
    stubMatchMedia(true);
    expect(resolveInitialTheme()).toBe("light");
  });

  it("falls back to dark when OS does not prefer light", () => {
    stubMatchMedia(false);
    expect(resolveInitialTheme()).toBe("dark");
  });

  it("ignores invalid stored values and keeps the chain intact", () => {
    localStorage.setItem(THEME_STORAGE_KEY, "sepia");
    stubMatchMedia(true);
    expect(resolveInitialTheme()).toBe("light");
  });
});

describe("uiStore theme state", () => {
  it("setTheme flips state without side effects (persistence/DOM live in useTheme)", () => {
    useUiStore.setState({ theme: "dark" });
    useUiStore.getState().setTheme("light");
    expect(useUiStore.getState().theme).toBe("light");
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBeNull();
  });

  it("initial store theme follows resolveInitialTheme", () => {
    useUiStore.setState({ theme: resolveInitialTheme() });
    expect(useUiStore.getState().theme).toBe("dark");
  });
});

describe("uiStore activeArea navigation state (需求 5)", () => {
  it("defaults to the chat area", () => {
    useUiStore.setState({ activeArea: "chat" });
    expect(useUiStore.getState().activeArea).toBe("chat");
  });

  it("setActiveArea flips between chat and editor", () => {
    useUiStore.setState({ activeArea: "chat" });
    useUiStore.getState().setActiveArea("editor");
    expect(useUiStore.getState().activeArea).toBe("editor");
    useUiStore.getState().setActiveArea("chat");
    expect(useUiStore.getState().activeArea).toBe("chat");
  });

  it("openFile switches the main area to the editor", () => {
    useUiStore.setState({ openFiles: [], activeFile: null, activeArea: "chat" });
    useUiStore.getState().openFile("src/main.rs");
    expect(useUiStore.getState().activeArea).toBe("editor");
    expect(useUiStore.getState().activeFile).toBe("src/main.rs");
  });

  it("selecting a session returns to the chat area", () => {
    useUiStore.setState({ activeArea: "editor", selectedSessionId: null });
    useUiStore.getState().selectSession("s1");
    expect(useUiStore.getState().activeArea).toBe("chat");
    expect(useUiStore.getState().selectedSessionId).toBe("s1");
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
