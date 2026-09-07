import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import {
  activateBuiltinEditorExtensions,
  getOverlays,
  getToolbarActions,
  useEditorExtVersion,
} from "../../lib/editor-ext";
import { ExtensionsPanel } from "../../lib/editor-ext/ExtensionsPanel";
import { FileTabs } from "./FileTabs";
import { FileTree } from "./FileTree";
import { MonacoTab } from "./MonacoTab";
import { WorkspaceForm } from "./WorkspaceForm";
import { useUiStore } from "../../lib/store/uiStore";

// Register + contribute the builtin editor extensions once at module load so
// preview matchers are queryable during the first child (MonacoTab) render.
// Idempotent: registerEditorExtension dedupes by id.
activateBuiltinEditorExtensions();

/**
 * Full-area workspace editor (nav rework, 需求 5): the editor is no longer a
 * right side panel — it occupies the same surface as the chat area and the
 * two are mutually exclusive (uiStore.activeArea). Hosts the extension
 * toolbar (插件化编辑器扩展, 需求 6): builtin toolbar actions, the extension
 * manager popover and extension overlays (floating panels).
 */
export function EditorArea(): ReactNode {
  const { t } = useTranslation();
  const openFiles = useUiStore((s) => s.openFiles);
  const activeFile = useUiStore((s) => s.activeFile);
  const workspaceQuery = useQuery({ queryKey: ["workspace"], queryFn: ipc.getWorkspace });
  const [switchOpen, setSwitchOpen] = useState(false);
  const root = workspaceQuery.data?.root ?? "";
  // Subscribe so contributed toolbar actions appear/disappear live when an
  // extension is toggled in the manager panel.
  useEditorExtVersion();

  return (
    <section aria-label={t("files.treeLabel")} className="flex min-h-0 flex-1 flex-col bg-surface-raised">
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
        {getToolbarActions().map(({ action }) => (
          <action.Component key={action.id} />
        ))}
        <ExtensionsPanel />
      </div>
      {switchOpen && (
        <div className="border-b border-ink-muted/30 bg-surface p-2">
          <WorkspaceForm initialRoot={root} onSuccess={() => setSwitchOpen(false)} onCancel={() => setSwitchOpen(false)} />
        </div>
      )}
      {openFiles.length > 0 && <FileTabs />}
      <div className="relative flex min-h-0 flex-1">
        <FileTree />
        {activeFile !== null && openFiles.includes(activeFile) ? (
          <MonacoTab key={activeFile} path={activeFile} />
        ) : (
          <div className="flex min-w-0 flex-1 items-center justify-center p-6 text-sm text-ink-muted">
            {t("editor.openFileHint")}
          </div>
        )}
        {getOverlays().map(({ overlay }) => (
          <overlay.Component key={overlay.id} />
        ))}
      </div>
    </section>
  );
}
