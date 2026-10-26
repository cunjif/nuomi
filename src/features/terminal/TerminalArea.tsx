/**
 * TerminalArea (ADR-0024 Part B, layout B2): the terminal area — mirrors the
 * workbench layout but replaces the Monaco editor column with xterm.js
 * terminals. The right aside (EditorToolbar + FileTree + WorkspaceBottomBar)
 * is shared with the workbench so the workspace directory panel is identical.
 *
 *   ┌──────────────────────┬──────────────────┐
 *   │ TerminalTabs         │ EditorToolbar    │
 *   │ xterm.js (flex-1)    │ FileTree         │
 *   │                      │ WorkspaceBottomBar│
 *   └──────────────────────┴──────────────────┘
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { FileTree } from "../shell/FileTree";
import { WorkspaceBottomBar } from "../shell/WorkspaceBottomBar";
import { EditorToolbar } from "../shell/EditorArea";
import { TerminalTabs } from "./TerminalTabs";
import { TerminalView } from "./TerminalView";
import { useUiStore, type TerminalInstance } from "../../lib/store/uiStore";

export function TerminalArea({ workspaceId }: { workspaceId: string }): ReactNode {
  const { t } = useTranslation();
  const bucket = useUiStore((s) => s.terminalByWorkspace[workspaceId]);
  const addTerminal = useUiStore((s) => s.addTerminal);

  const terminals = bucket?.terminals ?? [];
  const activeId = bucket?.activeTerminalId ?? null;
  const activeTerminal = terminals.find((t) => t.id === activeId) ?? null;

  const handleNew = (): void => {
    const id = `term-${Date.now()}`;
    const instance: TerminalInstance = {
      id,
      title: `Terminal ${terminals.length + 1}`,
      createdAt: Date.now(),
    };
    addTerminal(workspaceId, instance);
  };

  return (
    <section aria-label={t("terminal.areaLabel")} className="flex h-full min-h-0 flex-1 flex-col bg-surface-raised">
      <TerminalTabs workspaceId={workspaceId} />
      <div className="flex min-h-0 flex-1">
        {/* Terminal column: xterm.js owns its own viewport. */}
        <div className="relative flex min-h-0 min-w-0 flex-1 flex-col bg-surface">
          {activeTerminal !== null ? (
            <TerminalView key={activeTerminal.id} terminalId={activeTerminal.id} />
          ) : (
            <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-2 p-6 text-sm text-ink-muted">
              <p>{t("terminal.empty")}</p>
              <button
                type="button"
                onClick={handleNew}
                className="flex items-center gap-1 rounded border border-dashed border-ink-muted/40 px-3 py-1 text-xs text-ink-muted hover:bg-surface-overlay hover:text-ink-accent"
              >
                {t("terminal.new")}
              </button>
            </div>
          )}
        </div>
        {/* Shared right aside (identical to the workbench). */}
        <aside
          aria-label={t("files.treeLabel")}
          className="flex w-56 shrink-0 flex-col border-l border-ink-muted/30 bg-surface"
        >
          <EditorToolbar />
          <div className="min-h-0 flex-1 overflow-y-auto p-2">
            <FileTree workspaceId={workspaceId} />
          </div>
          <WorkspaceBottomBar workspaceId={workspaceId} />
        </aside>
      </div>
    </section>
  );
}
