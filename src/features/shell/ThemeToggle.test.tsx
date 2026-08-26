import { fireEvent, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { THEME_STORAGE_KEY, useUiStore } from "../../lib/store/uiStore";
import { stubLocalStorage } from "../../test/stubStorage";
import { renderWithProviders } from "../../test/helpers";
import { ThemeToggle } from "./ThemeToggle";

beforeEach(() => {
  stubLocalStorage();
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("ThemeToggle — left-rail dark/light switch", () => {
  it("clicking flips the root class, persists, and swaps the icon aria-label", () => {
    useUiStore.setState({ theme: "dark" });
    document.documentElement.classList.add("dark");
    renderWithProviders(<ThemeToggle />);

    // Dark → next stop is light (aria announces the target theme).
    const button = screen.getByRole("button", { name: "亮色" });
    expect(button).toHaveAttribute("title", "切换主题");
    fireEvent.click(button);
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("light");

    // Icon aria swapped with the theme.
    fireEvent.click(screen.getByRole("button", { name: "暗色" }));
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("dark");
    screen.getByRole("button", { name: "亮色" });
  });
});
