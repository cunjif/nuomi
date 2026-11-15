import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { stubLocalStorage } from "../../test/stubStorage";
import {
  PANEL_LAYOUT_STORAGE_KEY,
  PANEL_MIN_WIDTH,
  PANEL_MAX_WIDTH,
  PANEL_DEFAULT_WIDTH,
  clampPanelWidth,
  resolveInitialPanelLayout,
  useUiStore,
} from "./uiStore";

beforeEach(() => {
  stubLocalStorage();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("clampPanelWidth", () => {
  it("clamps to the configured bounds and rounds", () => {
    expect(clampPanelWidth(100)).toBe(PANEL_MIN_WIDTH);
    expect(clampPanelWidth(9999)).toBe(PANEL_MAX_WIDTH);
    expect(clampPanelWidth(224.6)).toBe(225);
    expect(clampPanelWidth(300)).toBe(300);
  });
});

describe("resolveInitialPanelLayout", () => {
  it("returns defaults when nothing is stored", () => {
    const layout = resolveInitialPanelLayout();
    expect(layout.leftPanel).toEqual({ width: PANEL_DEFAULT_WIDTH, collapsed: false, hidden: false });
    expect(layout.rightPanel).toEqual({ width: PANEL_DEFAULT_WIDTH, collapsed: false, hidden: false });
  });

  it("reads back a persisted layout", () => {
    localStorage.setItem(
      PANEL_LAYOUT_STORAGE_KEY,
      JSON.stringify({
        left: { width: 320, collapsed: true, hidden: false },
        right: { width: 260, collapsed: false, hidden: true },
      }),
    );
    const layout = resolveInitialPanelLayout();
    expect(layout.leftPanel).toEqual({ width: 320, collapsed: true, hidden: false });
    expect(layout.rightPanel).toEqual({ width: 260, collapsed: false, hidden: true });
  });

  it("falls back per panel on malformed entries", () => {
    localStorage.setItem(PANEL_LAYOUT_STORAGE_KEY, JSON.stringify({ left: { width: "x" }, right: 42 }));
    const layout = resolveInitialPanelLayout();
    expect(layout.leftPanel).toEqual({ width: PANEL_DEFAULT_WIDTH, collapsed: false, hidden: false });
    expect(layout.rightPanel).toEqual({ width: PANEL_DEFAULT_WIDTH, collapsed: false, hidden: false });
  });

  it("falls back to defaults on corrupted JSON", () => {
    localStorage.setItem(PANEL_LAYOUT_STORAGE_KEY, "not-json");
    const layout = resolveInitialPanelLayout();
    expect(layout.leftPanel.width).toBe(PANEL_DEFAULT_WIDTH);
    expect(layout.rightPanel.hidden).toBe(false);
  });

  it("clamps stored widths into the valid range", () => {
    localStorage.setItem(
      PANEL_LAYOUT_STORAGE_KEY,
      JSON.stringify({
        left: { width: 50, collapsed: false, hidden: false },
        right: { width: 9999, collapsed: false, hidden: false },
      }),
    );
    const layout = resolveInitialPanelLayout();
    expect(layout.leftPanel.width).toBe(PANEL_MIN_WIDTH);
    expect(layout.rightPanel.width).toBe(PANEL_MAX_WIDTH);
  });
});

describe("panel layout setters", () => {
  it("setLeftPanel patches the state and persists both panels", () => {
    useUiStore.getState().setLeftPanel({ collapsed: true });
    expect(useUiStore.getState().leftPanel.collapsed).toBe(true);
    const stored = JSON.parse(localStorage.getItem(PANEL_LAYOUT_STORAGE_KEY) ?? "{}") as {
      left: { collapsed: boolean };
      right: unknown;
    };
    expect(stored.left.collapsed).toBe(true);
    expect(stored.right).toEqual(useUiStore.getState().rightPanel);
  });

  it("setRightPanel clamps the width and persists", () => {
    useUiStore.getState().setRightPanel({ width: 9999 });
    expect(useUiStore.getState().rightPanel.width).toBe(PANEL_MAX_WIDTH);
    const stored = JSON.parse(localStorage.getItem(PANEL_LAYOUT_STORAGE_KEY) ?? "{}") as {
      right: { width: number };
    };
    expect(stored.right.width).toBe(PANEL_MAX_WIDTH);
  });

  it("a partial patch keeps the other panel untouched", () => {
    useUiStore.setState({ leftPanel: { width: 300, collapsed: false, hidden: false } });
    useUiStore.getState().setRightPanel({ hidden: true });
    expect(useUiStore.getState().leftPanel.width).toBe(300);
    expect(useUiStore.getState().rightPanel.hidden).toBe(true);
  });
});
