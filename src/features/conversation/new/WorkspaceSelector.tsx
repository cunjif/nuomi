import type { ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ipc } from "../../../lib/ipc/client";
import { useUiStore } from "../../../lib/store/uiStore";
import { workspaceBadgeColor } from "../workspaceBadgeColor";

export interface WorkspaceSelectorProps {
  selectedWorkspaceId: string | null;
  onSelect: (id: string | null) => void;
}

/**
 * Workspace picker for the new-conversation wizard. Loads the workspace list
 * asynchronously (skeleton during load), defaults to the focused workspace,
 * and disables selection when no workspaces are registered.
 */
export function WorkspaceSelector({ selectedWorkspaceId, onSelect }: WorkspaceSelectorProps): ReactNode {
  const { t } = useTranslation();
  const focusedWorkspaceId = useUiStore((s) => s.focusedWorkspaceId);

  const workspacesQuery = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => ipc.listWorkspaces(),
    staleTime: 30_000,
  });

  const workspaces = workspacesQuery.data ?? [];

  if (workspacesQuery.isLoading) {
    return (
      <div className="flex items-center gap-2 rounded border border-ink-muted/40 bg-surface-raised px-2 py-1.5">
        <div className="h-4 w-4 animate-pulse rounded-full bg-ink-muted/30" />
        <div className="h-3 w-32 animate-pulse rounded bg-ink-muted/30" />
      </div>
    );
  }

  if (workspaces.length === 0) {
    return (
      <div className="rounded border border-ink-muted/40 bg-surface-raised px-2 py-1.5 text-sm text-ink-muted">
        {t("conversation.workspaceSelectorNone")}
      </div>
    );
  }

  const effectiveSelected = selectedWorkspaceId ?? focusedWorkspaceId ?? workspaces[0]?.id ?? null;

  return (
    <div className="flex flex-col gap-1 rounded border border-ink-muted/40 bg-surface-raised p-2">
      <label className="text-xs text-ink-muted">{t("conversation.workspaceSelectorLabel")}</label>
      <div className="flex flex-col gap-0.5 max-h-32 overflow-y-auto">
        {workspaces.map((ws) => {
          const checked = effectiveSelected === ws.id;
          return (
            <button
              key={ws.id}
              type="button"
              onClick={() => onSelect(ws.id)}
              className={`flex items-center gap-2 rounded px-2 py-1.5 text-sm text-ink transition-colors ${checked ? "bg-ink-accent/15" : "hover:bg-surface-overlay"}`}
            >
              <span
                className="h-3 w-3 shrink-0 rounded-full"
                style={{ backgroundColor: workspaceBadgeColor(ws.rootPath) }}
              />
              <span className="flex-1 text-left truncate">{ws.rootPath}</span>
              <span
                className={`flex h-4 w-4 shrink-0 items-center justify-center rounded border ${checked ? "border-ink-accent bg-ink-accent text-surface" : "border-ink-muted/50"}`}
              >
                {checked ? "✓" : ""}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
