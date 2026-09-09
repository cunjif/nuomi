/**
 * Theme side effects: mirror the uiStore theme onto <html> via the
 * `data-theme` attribute (and the legacy `.dark` class for dark-family themes)
 * and persist the choice to localStorage. Components call this hook — never the
 * raw store action — so DOM/storage stay in sync (typescript-react rule: side
 * effects live in hooks, not render bodies).
 */
import { useEffect } from "react";
import { THEME_STORAGE_KEY, nextTheme, resolveInitialTheme, useUiStore, type Theme } from "./uiStore";

export interface UseTheme {
  theme: Theme;
  setTheme: (theme: Theme) => void;
  /** Cycles through the four themes (used by the legacy toggle affordance). */
  cycleTheme: () => void;
}

const DARK_FAMILY: ReadonlySet<Theme> = new Set(["chalkboard-dark", "high-contrast"]);

function applyTheme(theme: Theme): void {
  const root = document.documentElement;
  root.dataset.theme = theme;
  root.classList.toggle("dark", DARK_FAMILY.has(theme));
}

export function useTheme(): UseTheme {
  const theme = useUiStore((s) => s.theme);
  const setStoreTheme = useUiStore((s) => s.setTheme);

  // Keep the DOM honest on mount (covers environments without the index.html
  // inline script, e.g. tests or hot reloads).
  useEffect(() => {
    applyTheme(theme);
  }, [theme]);

  const setTheme = (next: Theme): void => {
    setStoreTheme(next);
    applyTheme(next);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, next);
    } catch {
      // Storage unavailable — theme still applies for this session.
    }
  };

  const cycleTheme = (): void => {
    setTheme(nextTheme(theme));
  };

  return { theme, setTheme, cycleTheme };
}

/** Re-exported for tests and future consumers. */
export { resolveInitialTheme };
