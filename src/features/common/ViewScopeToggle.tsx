import { useTranslation } from "react-i18next";

export type ViewSurface = "board" | "approvals" | "scheduler";
export type ViewScope = "focused" | "all";

export interface ViewScopeToggleProps {
  surface: ViewSurface;
  scope: ViewScope;
  onScopeChange: (scope: ViewScope) => void;
}

export function ViewScopeToggle({ surface, scope, onScopeChange }: ViewScopeToggleProps): React.ReactNode {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-1" data-surface={surface}>
      <button
        type="button"
        onClick={() => onScopeChange("focused")}
        className={`rounded px-2 py-0.5 text-xs transition-colors ${
          scope === "focused"
            ? "bg-ink-accent text-surface"
            : "bg-surface-raised text-ink-muted hover:text-ink"
        }`}
      >
        {t("common.viewScopeFocused")}
      </button>
      <button
        type="button"
        onClick={() => onScopeChange("all")}
        className={`rounded px-2 py-0.5 text-xs transition-colors ${
          scope === "all"
            ? "bg-ink-accent text-surface"
            : "bg-surface-raised text-ink-muted hover:text-ink"
        }`}
      >
        {t("common.viewScopeAll")}
      </button>
    </div>
  );
}
