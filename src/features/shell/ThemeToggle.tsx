import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useTheme } from "../../lib/store/useTheme";
import { THEME_ORDER, type Theme } from "../../lib/store/uiStore";
import { Icon, type IconName } from "../../components/ui/Icon/Icon";

const THEME_META: Record<Theme, { icon: IconName; labelKey: string }> = {
  "paper-light": { icon: "paper", labelKey: "theme.paperLight" },
  "grid-notebook": { icon: "grid", labelKey: "theme.gridNotebook" },
  "chalkboard-dark": { icon: "chalkboard", labelKey: "theme.chalkboardDark" },
  "high-contrast": { icon: "contrast", labelKey: "theme.highContrast" },
};

/**
 * Four-theme hand-drawn selector (review §9.3). The current theme shows in the
 * trigger; the panel is a native <details> popover (self-contained, no extra
 * state) listing all four with a hand-drawn thumbnail + check for the active.
 */
export function ThemeToggle(): ReactNode {
  const { t } = useTranslation();
  const { theme, setTheme } = useTheme();
  const current = THEME_META[theme];

  return (
    <details className="group relative">
      <summary
        className="flex w-full cursor-pointer list-none items-center justify-center gap-2 rounded px-3 py-1.5 text-sm text-ink-muted hover:bg-surface-overlay hover:text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
        aria-label={t("theme.toggle")}
        title={t("theme.toggle")}
      >
        <Icon name={current.icon} size={16} />
        <span>{t(current.labelKey)}</span>
      </summary>
      <div className="absolute bottom-full left-0 z-20 mb-1 w-48 rounded border border-ink-muted/60 bg-surface-raised p-1 shadow-sketch-md">
        {THEME_ORDER.map((key) => {
          const active = key === theme;
          const meta = THEME_META[key];
          const cls =
            "flex w-full items-center gap-2 rounded px-2 py-1.5 text-sm text-left transition-colors " +
            (active
              ? "bg-surface-overlay text-ink ring-1 ring-ink-accent"
              : "text-ink-muted hover:bg-surface-overlay hover:text-ink");
          return (
            <button key={key} type="button" onClick={() => setTheme(key)} className={cls}>
              <Icon name={meta.icon} size={16} />
              <span className="flex-1">{t(meta.labelKey)}</span>
              {active && <Icon name="check" size={14} />}
            </button>
          );
        })}
      </div>
    </details>
  );
}
