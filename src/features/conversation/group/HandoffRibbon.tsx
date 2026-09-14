import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

export interface HandoffRibbonProps {
  fromRole: string;
  toRole: string;
}

/** Handoff indicator ribbon between speaker bubbles. */
export function HandoffRibbon({ fromRole, toRole }: HandoffRibbonProps): ReactNode {
  const { t } = useTranslation();
  return (
    <div className="flex items-center gap-1 py-0.5 pl-3 text-xs text-ink-muted">
      <span>{fromRole}</span>
      <span aria-hidden="true">→</span>
      <span>{toRole}</span>
      <span className="ml-1 rounded bg-ink-muted/20 px-1">{t("conversation.handoff")}</span>
    </div>
  );
}
