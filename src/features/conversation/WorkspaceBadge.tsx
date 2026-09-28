import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { workspaceBadgeColor } from "./workspaceBadgeColor";

export interface WorkspaceBadgeProps {
  /** Workspace root path (null when the workspace is unknown or unaffiliated). */
  workspaceRootPath: string | null;
  /** Workspace id (used to detect the `__migrated__` sentinel). */
  workspaceId: string;
}

/** Sentinel value for sessions that predate the workspace_id column. */
export const MIGRATED_WORKSPACE_ID = "__migrated__";

/**
 * Color dot + label for a session's workspace. Unaffiliated sessions
 * (`__migrated__` or unknown workspace) render a neutral "未归属" badge.
 */
export function WorkspaceBadge({ workspaceRootPath, workspaceId }: WorkspaceBadgeProps): ReactNode {
  const { t } = useTranslation();

  if (workspaceId === MIGRATED_WORKSPACE_ID || workspaceRootPath === null) {
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

  const color = workspaceBadgeColor(workspaceRootPath);
  return (
    <span
      className="inline-flex items-center gap-1 rounded px-1 py-0.5 text-xs text-ink-muted"
      title={workspaceRootPath}
    >
      <span className="h-2 w-2 rounded-full" style={{ backgroundColor: color }} />
    </span>
  );
}
