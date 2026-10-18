import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { workspaceBadgeColor } from "./workspaceBadgeColor";

export interface WorkspaceBadgeProps {
  /** Workspace label text (full variant preferred; null when the workspace is unknown or unaffiliated). */
  workspaceLabel: string | null;
  /** Workspace id (hashed for the color dot; `__migrated__` marks legacy rows). */
  workspaceId: string;
}

/** Sentinel value for sessions that predate the workspace_id column. */
export const MIGRATED_WORKSPACE_ID = "__migrated__";

/**
 * Color dot + tooltip for a session's workspace. The dot color hashes the
 * stable workspace id (ADR-0021); the tooltip shows the workspace label.
 * Unaffiliated sessions (`__migrated__` or unknown workspace) render a
 * neutral "未归属" badge.
 */
export function WorkspaceBadge({ workspaceLabel, workspaceId }: WorkspaceBadgeProps): ReactNode {
  const { t } = useTranslation();

  if (workspaceId === MIGRATED_WORKSPACE_ID || workspaceLabel === null) {
    return (
      <span
        className="inline-flex items-center gap-1 rounded px-1 py-0.5 text-xs text-ink-muted bg-ink-muted/15"
        title={t("conversation.workspaceBadgeUnaffiliated")}
      >
        <span className="h-2 w-2 rounded-full bg-ink-muted/50" />
        {t("conversation.workspaceBadgeUnaffiliated")}
      </span>
    );
  }

  const color = workspaceBadgeColor(workspaceId);
  return (
    <span
      className="inline-flex items-center gap-1 rounded px-1 py-0.5 text-xs text-ink-muted"
      title={workspaceLabel}
    >
      <span className="h-2 w-2 rounded-full" style={{ backgroundColor: color }} />
    </span>
  );
}
