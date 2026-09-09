import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { stubLocalStorage } from "../../test/stubStorage";
import { THEME_STORAGE_KEY, useUiStore } from "./uiStore";
import { useTheme } from "./useTheme";

const html = document.documentElement;

beforeEach(() => {
  stubLocalStorage();
  html.classList.remove("dark");
  delete html.dataset.theme;
  // Simulate the index.html inline script outcome for the dark default.
  useUiStore.setState({ theme: "chalkboard-dark" });
  html.classList.add("dark");
  html.dataset.theme = "chalkboard-dark";
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("useTheme — persistence + root attribute side effects", () => {
  it("cycle persists to localStorage and flips the dark class both ways", () => {
    const { result } = renderHook(() => useTheme());
    expect(result.current.theme).toBe("chalkboard-dark");

    act(() => result.current.cycleTheme());
    expect(useUiStore.getState().theme).toBe("high-contrast");
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("high-contrast");
    expect(html.dataset.theme).toBe("high-contrast");
    expect(html.classList.contains("dark")).toBe(true);

    act(() => result.current.cycleTheme());
    expect(useUiStore.getState().theme).toBe("paper-light");
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("paper-light");
    expect(html.dataset.theme).toBe("paper-light");
    expect(html.classList.contains("dark")).toBe(false);
  });

  it("setTheme applies an explicit theme and stays idempotent", () => {
    const { result } = renderHook(() => useTheme());
    act(() => result.current.setTheme("paper-light"));
    act(() => result.current.setTheme("paper-light"));
    expect(useUiStore.getState().theme).toBe("paper-light");
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("paper-light");
    expect(html.dataset.theme).toBe("paper-light");
    expect(html.classList.contains("dark")).toBe(false);
  });

  it("syncs a mismatched root class on mount", () => {
    useUiStore.setState({ theme: "paper-light" });
    html.classList.add("dark");
    expect(html.classList.contains("dark")).toBe(true);
    renderHook(() => useTheme());
    expect(html.classList.contains("dark")).toBe(false);
  });
});
