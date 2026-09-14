import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { ContextInjectionDto } from "../../../lib/ipc/client";

export interface ContextInjectionBarProps {
  injections: ContextInjectionDto[];
  onRemove: (id: string) => void;
}

/**
 * Shows active context injection markers above the composer.
 * Each marker displays the injection type and supports removal.
 */
export function ContextInjectionBar({ injections, onRemove }: ContextInjectionBarProps): ReactNode {
  const { t } = useTranslation();

  if (injections.length === 0) return null;

  const labelFor = (type: string): string => {
    if (type === "session_ref") return t("composer.injection.sessionRef");
    if (type === "rule") return t("composer.injection.rule");
    if (type === "custom_prompt") return t("composer.injection.customPrompt");
    return type;
  };

  return (
    <div className="flex flex-wrap gap-1 pb-1" role="list" aria-label={t("composer.injection.label")}>
      {injections.map((inj) => (
        <div
          key={inj.id}
          role="listitem"
          className="flex items-center gap-1 rounded border border-ink-accent/30 bg-ink-accent/10 px-1.5 py-0.5 text-xs text-ink"
        >
          <span>{labelFor(inj.type)}</span>
          <button
            type="button"
            onClick={() => onRemove(inj.id)}
            className="text-ink-muted hover:text-ink"
            aria-label={t("common.remove")}
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
