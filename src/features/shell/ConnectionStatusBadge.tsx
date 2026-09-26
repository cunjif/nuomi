import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useConnectionStatus } from "./useConnectionStatus";

/**
 * Connection status indicator: colored dot + i18n label.
 * Shared by AreaNav row 1 and ChatTabBar so both reflect the same probe.
 */
export function ConnectionStatusBadge(): ReactNode {
  const { t } = useTranslation();
  const { status, color } = useConnectionStatus();
  return (
    <span className="flex items-center gap-2 text-xs text-ink-muted" role="status">
      <span aria-hidden="true" className={`inline-block size-2 rounded-full ${color}`} />
      {t(`shell.${status}`)}
    </span>
  );
}
