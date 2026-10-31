import { fireEvent, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { THEME_STORAGE_KEY, useUiStore } from "../../lib/store/uiStore";
import { stubLocalStorage } from "../../test/stubStorage";
import { renderWithProviders } from "../../test/helpers";
import { ThemeToggle } from "./ThemeToggle";

beforeEach(() => {
  stubLocalStorage();
  document.documentElement.classList.remove("dark");
  delete document.documentElement.dataset.theme;
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("ThemeToggle — four-theme selector", () => {
  it("selecting a theme sets data-theme, toggles .dark for the dark family, and persists", () => {
    useUiStore.setState({ theme: "chalkboard-dark" });
    document.documentElement.classList.add("dark");
    document.documentElement.dataset.theme = "chalkboard-dark";
    renderWithProviders(<ThemeToggle />);

    // Open the selector via its trigger button, then pick Paper Light (light family).
    const trigger = screen.getByRole("button", { name: /切换主题/ });
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("button", { name: /纸质亮/ }));
    expect(document.documentElement.dataset.theme).toBe("paper-light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("paper-light");

    // Panel auto-closes after selecting; reopen, then pick High Contrast (dark family keeps .dark).
    fireEvent.click(trigger);
    fireEvent.click(screen.getByRole("button", { name: /高对比/ }));
    expect(document.documentElement.dataset.theme).toBe("high-contrast");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe("high-contrast");
  });
});
