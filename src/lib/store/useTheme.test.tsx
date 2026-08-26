import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { stubLocalStorage } from "../../test/stubStorage";
import { THEME_STORAGE_KEY, useUiStore } from "./uiStore";
import { useTheme } from "./useTheme";

const html = document.documentElement;

beforeEach(() => {
  stubLocalStorage();
  html.classList.remove("dark");
  // Simulate the index.html inline script outcome for the dark default.
  useUiStore.setState({ theme: "dark" });
  html.classList.add("dark");
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("useTheme — persistence + root class side effects", () => {
  it("toggle persists to localStorage and flips the dark class both ways", () => {
    const { result } = renderHook(() => useTheme());
    expect(result.current.theme).toBe("dark");

    act(() => result.current.toggleTheme());
    expect(useUiStore.getState().theme).toBe("light");
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");
    expect(html.classList.contains("dark")).toBe(false);

    act(() => result.current.toggleTheme());
    expect(useUiStore.getState().theme).toBe("dark");
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
    expect(html.classList.contains("dark")).toBe(true);
  });

  it("setTheme applies an explicit theme and stays idempotent", () => {
    const { result } = renderHook(() => useTheme());
    act(() => result.current.setTheme("light"));
    act(() => result.current.setTheme("light"));
    expect(useUiStore.getState().theme).toBe("light");
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");
    expect(html.classList.contains("dark")).toBe(false);
  });

  it("syncs a mismatched root class on mount", () => {
    useUiStore.setState({ theme: "light" });
    expect(html.classList.contains("dark")).toBe(true);
    renderHook(() => useTheme());
    expect(html.classList.contains("dark")).toBe(false);
  });
});
