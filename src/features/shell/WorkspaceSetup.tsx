import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { WorkspaceForm } from "./WorkspaceForm";

/**
 * First-launch workspace setup: shown when no workspace has been configured
 * yet (no persisted root and no `NUOMI_WORKSPACE_ROOT` env). Blocks the main
 * shell until a sandbox root is confirmed.
 */
export function WorkspaceSetup(): ReactNode {
  const { t } = useTranslation();
  const workspaceQuery = useQuery({ queryKey: ["workspace"], queryFn: ipc.getWorkspace });

  return (
    <div className="flex h-screen items-center justify-center bg-surface text-ink">
      <div className="w-96 rounded-lg border border-ink-muted/30 bg-surface-raised p-6 shadow-lg">
        <h1 className="text-lg font-semibold">{t("workspace.setupTitle")}</h1>
        <p className="mt-2 text-sm text-ink-muted">{t("workspace.setupDescription")}</p>
        <div className="mt-4">
          {/* After a successful save the ["workspace"] query refetches with
              configured=true and the shell renders the main layout. */}
          <WorkspaceForm initialRoot={workspaceQuery.data?.root ?? ""} />
        </div>
      </div>
    </div>
  );
}
