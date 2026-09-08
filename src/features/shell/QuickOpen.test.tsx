/**
 * QuickOpen palette tests: Ctrl+P file quick open (filter + Enter opens),
 * Ctrl+Shift+P command palette (builtin commands run), Escape dismiss and
 * the global Ctrl+F find-in-file interception.
 */
import type { ReactNode } from "react";
import { fireEvent, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { tdSeedFiles } from "../../lib/ipc/test-double";
import { renderWithProviders } from "../../test/helpers";
import { stubLocalStorage } from "../../test/stubStorage";
import { registerActiveEditor } from "../../lib/editor-ext/indexer/workspace-index";
import { usePaletteStore } from "../../lib/store/paletteStore";
import { THEME_STORAGE_KEY, useUiStore } from "../../lib/store/uiStore";
import { QuickOpen, useGlobalPaletteShortcuts } from "./QuickOpen";

function Harness(): ReactNode {
  useGlobalPaletteShortcuts();
  return <QuickOpen />;
}

function pressKey(init: KeyboardEventInit): void {
  // fireEvent (not raw dispatchEvent) so React flushes the zustand-driven
  // re-render synchronously inside act.
  fireEvent.keyDown(window, init);
}

beforeEach(() => {
  stubLocalStorage();
  // Module-global active editor: reset per test so a failing case can never
  // leak a registration into the next one.
  registerActiveEditor(null, () => {});
  usePaletteStore.setState({ mode: null });
  useUiStore.setState({ openFiles: [], activeFile: null, activeArea: "chat", theme: "dark", dirtyPaths: {} });
  localStorage.removeItem(THEME_STORAGE_KEY);
});

describe("QuickOpen — Ctrl+P file quick open", () => {
  it("lists workspace files, filters by query and opens on Enter", async () => {
    tdSeedFiles({ "src/a.ts": "const a = 1;", "README.md": "# hi", "src/deep/b.ts": "const b = 2;" });
    renderWithProviders(<Harness />);
    expect(screen.queryByTestId("quick-open")).toBeNull();

    pressKey({ code: "KeyP", ctrlKey: true });
    expect(screen.getByTestId("quick-open")).toBeTruthy();
    // Empty query lists files (basename + path spans may share the text).
    expect((await screen.findAllByText("a.ts")).length).toBeGreaterThan(0);

    // Typing filters (substring over the path).
    const input = screen.getByTestId("quick-open-input");
    fireEvent.change(input, { target: { value: "read" } });
    expect((await screen.findAllByText("README.md")).length).toBeGreaterThan(0);
    expect(screen.queryAllByText("a.ts")).toHaveLength(0);

    // Enter opens the selected file and closes the palette.
    fireEvent.keyDown(input, { key: "Enter", cancelable: true });
    expect(useUiStore.getState().activeFile).toBe("README.md");
    expect(useUiStore.getState().activeArea).toBe("editor");
    expect(screen.queryByTestId("quick-open")).toBeNull();
  });

  it("Ctrl+P toggles: second press closes", () => {
    tdSeedFiles({ "a.ts": "x" });
    renderWithProviders(<Harness />);
    pressKey({ code: "KeyP", ctrlKey: true });
    expect(screen.getByTestId("quick-open")).toBeTruthy();
    pressKey({ code: "KeyP", ctrlKey: true });
    expect(screen.queryByTestId("quick-open")).toBeNull();
  });

  it("Escape closes the palette", async () => {
    tdSeedFiles({ "a.ts": "x" });
    renderWithProviders(<Harness />);
    pressKey({ code: "KeyP", ctrlKey: true });
    expect(screen.getByTestId("quick-open")).toBeTruthy();
    fireEvent.keyDown(screen.getByTestId("quick-open-input"), { key: "Escape", cancelable: true });
    expect(screen.queryByTestId("quick-open")).toBeNull();
  });
});

describe("QuickOpen — Ctrl+Shift+P command palette", () => {
  it("lists builtin commands and runs the theme toggle", () => {
    renderWithProviders(<Harness />);
    pressKey({ code: "KeyP", ctrlKey: true, shiftKey: true });
    expect(screen.getByTestId("quick-open")).toBeTruthy();

    const toggle = screen.getByRole("option", { name: "切换深浅色主题" });
    fireEvent.click(toggle);
    expect(useUiStore.getState().theme).toBe("light");
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");
    expect(screen.queryByTestId("quick-open")).toBeNull();
  });

  it("runs 转到文件… which flips the palette into file mode", () => {
    tdSeedFiles({ "a.ts": "x" });
    renderWithProviders(<Harness />);
    pressKey({ code: "KeyP", ctrlKey: true, shiftKey: true });
    fireEvent.click(screen.getByRole("option", { name: /转到文件…/ }));
    // Still open, but now listing files.
    expect(screen.getByTestId("quick-open")).toBeTruthy();
    expect(screen.getByTestId("quick-open-input").getAttribute("placeholder")).toBe("查找工作区文件…");
  });
});

describe("QuickOpen — Ctrl+F find in file", () => {
  it("intercepts the chord when a Monaco surface registered a find action", () => {
    const find = vi.fn();
    registerActiveEditor("a.ts", () => {}, { find });
    renderWithProviders(<Harness />);
    pressKey({ code: "KeyF", ctrlKey: true });
    expect(find).toHaveBeenCalledTimes(1);
  });

  it("falls through (no interception) when no editor is live", () => {
    const spy = vi.spyOn(KeyboardEvent.prototype, "preventDefault");
    renderWithProviders(<Harness />);
    pressKey({ code: "KeyF", ctrlKey: true });
    expect(spy).not.toHaveBeenCalled();
    spy.mockRestore();
  });
});
