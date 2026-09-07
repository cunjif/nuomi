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
 * Workspace toolbar hosted in the AreaNav strip's right slot (VSCode-style:
 * workspace actions share the top navigation row, right-aligned). Shows the
 * sandbox root, the switch action and every toolbar action contributed by
 * editor extensions.
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
    <>
      <span className="max-w-44 truncate text-xs text-ink-muted">
        <span className="mr-1 font-semibold uppercase tracking-wide">{t("workspace.current")}</span>
        <span className="font-mono" title={root}>
          {root}
        </span>
      </span>
      <button
        type="button"
        onClick={() => setSwitchOpen((o) => !o)}
        className="shrink-0 rounded border border-ink-muted/40 px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
      >
        {t("workspace.switchWorkspace")}
      </button>
      {switchOpen && (
        <div className="absolute inset-y-0 right-2 z-20 flex items-center">
          <div className="rounded border border-ink-muted/40 bg-surface p-2 shadow-lg">
            <WorkspaceForm initialRoot={root} onSuccess={() => setSwitchOpen(false)} onCancel={() => setSwitchOpen(false)} />
          </div>
        </div>
      )}
      {getToolbarActions().map(({ action }) => (
        <action.Component key={action.id} />
      ))}
      <ExtensionsPanel />
    </>
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
      <div className="flex min-h-0 flex-1">
        {/* Editor column: breadcrumb header + Monaco + status live inside
        MonacoTab; the column itself never scrolls (Monaco owns its own
        viewport). Overlays (extension floating panels) anchor here. */}
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
        {/* Explorer sidebar (right, per 用户截图): open-file tabs on top, the
        fixed 工作区文件 header below them, then ONE scroll container for the
        whole tree. */}
        <aside
          aria-label={t("files.treeLabel")}
          className="flex w-56 shrink-0 flex-col border-l border-ink-muted/30 bg-surface"
        >
          <FileTabs />
          <div className="flex shrink-0 items-center justify-between border-b border-ink-muted/30 px-2 py-1.5">
            <h2 className="text-xs font-semibold uppercase tracking-wide text-ink-muted">{t("files.treeLabel")}</h2>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto p-2">
            <FileTree />
          </div>
        </aside>
      </div>
    </section>
  );
}
