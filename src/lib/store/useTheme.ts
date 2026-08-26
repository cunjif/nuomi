/**
 * Theme side effects: mirror the uiStore theme onto documentElement's "dark"
 * class and persist the choice to localStorage. Components call this hook —
 * never the raw store action — so DOM/storage stay in sync (typescript-react
 * rule: side effects live in hooks, not render bodies).
 */
import { useEffect } from "react";
import { THEME_STORAGE_KEY, resolveInitialTheme, useUiStore } from "./uiStore";

export interface UseTheme {
  theme: "dark" | "light";
  setTheme: (theme: "dark" | "light") => void;
  toggleTheme: () => void;
}

function applyThemeClass(theme: "dark" | "light"): void {
  document.documentElement.classList.toggle("dark", theme === "dark");
}

export function useTheme(): UseTheme {
  const theme = useUiStore((s) => s.theme);
  const setStoreTheme = useUiStore((s) => s.setTheme);

  // Keep the DOM class honest on mount (covers environments without the
  // index.html inline script, e.g. tests or hot reloads).
  useEffect(() => {
    applyThemeClass(theme);
  }, [theme]);

  const setTheme = (next: "dark" | "light"): void => {
    setStoreTheme(next);
    applyThemeClass(next);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, next);
    } catch {
      // Storage unavailable — theme still applies for this session.
    }
  };

  const toggleTheme = (): void => {
    setTheme(theme === "dark" ? "light" : "dark");
  };

  return { theme, setTheme, toggleTheme };
}

/** Re-exported for tests and future consumers. */
export { resolveInitialTheme };
