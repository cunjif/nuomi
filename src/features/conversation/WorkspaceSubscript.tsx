import type { ReactNode } from "react";

export interface WorkspaceSubscriptProps {
  workspaceName: string | null;
  onOpenWorkspaceList: () => void;
}

/**
 * Workspace name as a subscript on the conversation title: `(workspaceName)`
 * in muted xs text. Returns null when name is absent (avoids empty parens).
 * Click opens the workspace list dialog (preserves WorkspaceBar entry point).
 */
export function WorkspaceSubscript({ workspaceName, onOpenWorkspaceList }: WorkspaceSubscriptProps): ReactNode {
  if (!workspaceName || workspaceName.trim() === "") return null;
  return (
    <button
      type="button"
      onClick={onOpenWorkspaceList}
      className="shrink-1 max-w-40 truncate rounded px-1 text-xs text-ink-muted hover:bg-surface-overlay hover:text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
      title={workspaceName}
    >
      ({workspaceName})
    </button>
  );
}
