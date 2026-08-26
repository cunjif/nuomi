import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useTheme } from "../../lib/store/useTheme";

function SunIcon(): ReactNode {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="size-4">
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
    </svg>
  );
}

function MoonIcon(): ReactNode {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" className="size-4">
      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
    </svg>
  );
}

/**
 * Dark/light switch for the left-rail footer. Icon mirrors the current theme
 * (sun in light, moon in dark); aria-label names the theme you switch TO so
 * it changes on toggle.
 */
export function ThemeToggle(): ReactNode {
  const { t } = useTranslation();
  const { theme, toggleTheme } = useTheme();
  const nextLabel = theme === "dark" ? t("theme.light") : t("theme.dark");
  return (
    <button
      type="button"
      onClick={toggleTheme}
      aria-label={nextLabel}
      title={t("theme.toggle")}
      className="flex w-full items-center justify-center gap-2 rounded px-3 py-1.5 text-sm text-ink-muted hover:bg-surface-overlay hover:text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
    >
      {theme === "dark" ? <MoonIcon /> : <SunIcon />}
      <span>{nextLabel}</span>
    </button>
  );
}
