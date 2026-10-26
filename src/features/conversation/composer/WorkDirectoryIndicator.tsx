import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Icon } from "../../../components/ui/Icon/Icon";
import { useOpenWorkspace } from "../../../lib/store/workspaceQueries";

/**
 * Work-directory indicator shown at the composer's bottom-left, on the
 * same row as ContextUsageIndicator (chat_rich_bind 6.4): resolves the
 * session's bound `workDirectoryId` against the open workspace's
 * directories and renders the display name with the absolute path as
 * tooltip. Hidden when the session is unbound or the directory cannot be
 * resolved (e.g. cross-workspace legacy session under the "all" filter).
 */
export function WorkDirectoryIndicator({
  workDirectoryId,
}: {
  workDirectoryId: string | null;
}): ReactNode {
  const { t } = useTranslation();
  const { data: open } = useOpenWorkspace();

  if (workDirectoryId === null || open === undefined) return null;

  const dir = (open?.directories ?? []).find((d) => d.id === workDirectoryId);
  if (dir === undefined) return null;

  return (
    <span
      className="mr-auto flex max-w-[50%] items-center gap-1 truncate text-xs text-ink-muted"
      title={dir.absPath}
      aria-label={t("composer.workDirectory")}
    >
      <Icon name="file" size={12} className="shrink-0" />
      <span className="truncate">{dir.displayName}</span>
    </span>
  );
}
