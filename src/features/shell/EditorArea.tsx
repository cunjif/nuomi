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
import { WorkspaceListDialog } from "./WorkspaceListDialog";
import { useUiStore } from "../../lib/store/uiStore";

// Register + contribute the builtin editor extensions once at module load so
// preview matchers are queryable during the first child (MonacoTab) render.
// Idempotent: registerEditorExtension dedupes by id.
activateBuiltinEditorExtensions();

/**
 * Sidebar-hosted workspace toolbar (用户截图布局): the actions that used to
 * share the top nav strip now live at the top of the explorer sidebar —
 * [切换工作区] + every toolbar action contributed by editor extensions
 * (注释 / 扩展 …), with the sandbox root shown on the row below.
 */
export function EditorToolbar(): ReactNode {
  const { t } = useTranslation();
  const workspaceQuery = useQuery({ queryKey: ["workspace"], queryFn: ipc.getWorkspace });
  const [switchOpen, setSwitchOpen] = useState(false);
  const root = workspaceQuery.data?.root ?? "";
  // Subscribe so contributed toolbar actions appear/disappear live when an
  // extension is toggled in the manager panel.
  useEditorExtVersion();

  return (
    <div className="shrink-0 border-b border-ink-muted/30 bg-surface">
      <div className="flex items-center gap-1 p-1.5">
        <button
          type="button"
          onClick={() => setSwitchOpen(true)}
          className="sketch-btn px-2 py-0.5 text-xs text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
        >
          {t("workspace.switchWorkspace")}
        </button>
        {getToolbarActions().map(({ action }) => (
          <action.Component key={action.id} />
        ))}
        <ExtensionsPanel />
      </div>
      {/* 切换工作区 now opens the modal dialog (用户 SVG one.svg) instead of
      the inline sidebar form. */}
      <WorkspaceListDialog open={switchOpen} onClose={() => setSwitchOpen(false)} />
      <div className="flex items-center gap-1 border-t border-ink-muted/30 px-2 py-1 text-xs">
        <span className="shrink-0 font-semibold uppercase tracking-wide text-ink-muted">{t("workspace.current")}</span>
        <span className="min-w-0 truncate font-mono text-ink-muted" title={root}>
          {root}
        </span>
      </div>
    </div>
  );
}

/**
 * Full-area workspace editor (nav rework, 需求 5), laid out following the
 * VSCode editor model (用户截图为准确布局):
 *
 *   ┌──────────────────────────────┬──────────────┐
 *   │ breadcrumb row + 保存(Ctrl+S) │ open tabs    │
 *   │ Monaco (own scroll)          │ 工作区文件    │
 *   │ status row                   │ tree (scroll)│
 *   └──────────────────────────────┴──────────────┘
 *
 * Explorer sidebar sits on the right of the editor column; the two panels
 * scroll independently. Scroll discipline (VSCode 设计理念): the page never
 * scrolls — every panel owns exactly one scroll container and every flex
 * ancestor carries min-h-0 so the inner heights are definite.
 */
export function EditorArea(): ReactNode {
  const { t } = useTranslation();
  const openFiles = useUiStore((s) => s.openFiles);
  const activeFile = useUiStore((s) => s.activeFile);

  return (
    <section aria-label={t("files.treeLabel")} className="flex h-full min-h-0 flex-1 flex-col bg-surface-raised">
      {/* Open-file tab strip: directly below the 对话|文件编辑 nav row
      (用户布局), spanning the editor panel. */}
      {openFiles.length > 0 && <FileTabs />}
      <div className="flex min-h-0 flex-1">
        {/* Editor column: Monaco header/body/status live inside MonacoTab;
        the column itself never scrolls (Monaco owns its own viewport).
        Overlays (extension floating panels) anchor here. */}
        <div className="relative flex min-h-0 min-w-0 flex-1 flex-col">
          {activeFile !== null && openFiles.includes(activeFile) ? (
            <MonacoTab key={activeFile} path={activeFile} />
          ) : (
            <div className="flex min-h-0 flex-1 items-center justify-center p-6 text-sm text-ink-muted">
              {t("editor.openFileHint")}
            </div>
          )}
          {getOverlays().map(({ overlay }) => (
            <overlay.Component key={overlay.id} />
          ))}
        </div>
        {/* Explorer sidebar (right, per 用户截图): workspace actions + root
        label on top, then ONE scroll container for the whole tree. */}
        <aside
          aria-label={t("files.treeLabel")}
          className="flex w-56 shrink-0 flex-col border-l border-ink-muted/30 bg-surface"
        >
          <EditorToolbar />
          <div className="min-h-0 flex-1 overflow-y-auto p-2">
            <FileTree />
          </div>
        </aside>
      </div>
    </section>
  );
}
