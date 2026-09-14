import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";

export interface NextRunCountdownProps {
  nextTriggerAt: number | null;
}

/** Live countdown to the next schedule trigger. */
export function NextRunCountdown({ nextTriggerAt }: NextRunCountdownProps): ReactNode {
  const { t } = useTranslation();
  if (nextTriggerAt === null) {
    return <span className="text-xs text-ink-muted">{t("conversation.noNextRun")}</span>;
  }
  const remaining = nextTriggerAt - Date.now();
  if (remaining <= 0) {
    return <span className="text-xs text-ink-accent animate-pulse">{t("conversation.triggering")}</span>;
  }
  const hours = Math.floor(remaining / 3_600_000);
  const minutes = Math.floor((remaining % 3_600_000) / 60_000);
  const seconds = Math.floor((remaining % 60_000) / 1_000);
  return (
    <span className="text-xs tabular-nums text-ink-muted">
      {t("conversation.nextRunIn")}: {hours > 0 && `${hours}h `}{minutes}m {seconds}s
    </span>
  );
}
