import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { FileTabs } from "./FileTabs";
import { FileTree } from "./FileTree";
import { MonacoTab } from "./MonacoTab";
import { WorkspaceForm } from "./WorkspaceForm";
import { useUiStore } from "../../lib/store/uiStore";

/**
 * U10 right pane: workspace tree + open-file tabs + lazy Monaco editor.
 * Widens once a tab is open so the editor stays usable. The title bar shows
 * the active workspace root and offers a switch dialog.
 */
export function FilePanel(): ReactNode {
  const { t } = useTranslation();
  const openFiles = useUiStore((s) => s.openFiles);
  const activeFile = useUiStore((s) => s.activeFile);
  const workspaceQuery = useQuery({ queryKey: ["workspace"], queryFn: ipc.getWorkspace });
  const [switchOpen, setSwitchOpen] = useState(false);
  const root = workspaceQuery.data?.root ?? "";

  return (
    <aside
      className={`flex min-h-0 shrink-0 flex-col border-l border-ink-muted/30 bg-surface-raised transition-[width] ${
        openFiles.length > 0 ? "w-[42rem]" : "w-60"
      }`}
    >
      <div className="flex min-w-0 items-center gap-1 border-b border-ink-muted/30 px-2 py-1.5">
        <span className="shrink-0 text-xs font-semibold uppercase tracking-wide text-ink-muted">
          {t("workspace.current")}
        </span>
        <span title={root} className="min-w-0 flex-1 truncate font-mono text-xs text-ink">
          {root}
        </span>
        <button
          type="button"
          onClick={() => setSwitchOpen(true)}
          className="shrink-0 rounded border border-ink-muted/40 px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
        >
          {t("workspace.switchWorkspace")}
        </button>
      </div>
      {switchOpen && (
        <div className="border-b border-ink-muted/30 bg-surface p-2">
          <WorkspaceForm initialRoot={root} onSuccess={() => setSwitchOpen(false)} onCancel={() => setSwitchOpen(false)} />
        </div>
      )}
      {openFiles.length > 0 && <FileTabs />}
      <div className="flex min-h-0 flex-1">
        <FileTree />
        {activeFile !== null && openFiles.includes(activeFile) && <MonacoTab key={activeFile} path={activeFile} />}
      </div>
    </aside>
  );
}
