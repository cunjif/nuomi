/**
 * TerminalTabs (ADR-0024 Part B): multi-terminal tab strip, mirrors the
 * workbench FileTabs pattern. Each tab is a terminal instance in the
 * per-workspace bucket; [+] spawns a new one, × kills it.
 *
 * Tabs are drag-reorderable via native HTML5 DnD (drag the tab onto another
 * tab) — a pure display-order change, the PTY session keeps running. When a
 * drop is detected mid-rename the input simply loses its drag handle:
 * `draggable` is off while renaming.
 */
import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { ipc } from "../../lib/ipc/client";
import { useUiStore, type TerminalInstance } from "../../lib/store/uiStore";
import { Icon } from "../../components/ui/Icon/Icon";
import { TerminalCwdBadge } from "./TerminalCwdBadge";
import { resolveTerminalCwd } from "./resolveTerminalCwd";
import { nextTerminalTitle } from "./terminalTitle";

const DRAG_MIME = "text/terminal-id";

export function TerminalTabs({ workspaceId }: { workspaceId: string }): ReactNode {
  const { t } = useTranslation();
  const bucket = useUiStore((s) => s.terminalByWorkspace[workspaceId]);
  const addTerminal = useUiStore((s) => s.addTerminal);
  const removeTerminal = useUiStore((s) => s.removeTerminal);
  const setActiveTerminal = useUiStore((s) => s.setActiveTerminal);
  const renameTerminal = useUiStore((s) => s.renameTerminal);
  const reorderTerminal = useUiStore((s) => s.reorderTerminal);

  const terminals = bucket?.terminals ?? [];
  const activeId = bucket?.activeTerminalId ?? null;

  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (renamingId === null) return;
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [renamingId]);

  const handleNew = async (): Promise<void> => {
    const { cwd, source } = await resolveTerminalCwd(workspaceId);
    const id = `term-${Date.now()}`;
    const instance: TerminalInstance = {
      id,
      title: nextTerminalTitle(terminals),
      createdAt: Date.now(),
      cwd,
      cwdSource: source,
    };
    addTerminal(workspaceId, instance);
  };

  const handleClose = (terminalId: string): void => {
    // Kill the PTY by its backend id — the tab key (`term-<timestamp>`) is a
    // frontend-only handle and would fail with `terminal.not_found`, leaking
    // the shell process.
    const term = terminals.find((t) => t.id === terminalId);
    const backendId = term?.backendId ?? term?.id ?? terminalId;
    void ipc.terminalKill(backendId).catch(() => { /* already gone */ });
    removeTerminal(workspaceId, terminalId);
  };

  const startRename = (term: TerminalInstance): void => {
    setDraft(term.title);
    setRenamingId(term.id);
  };

  const commitRename = (): void => {
    if (renamingId !== null) renameTerminal(workspaceId, renamingId, draft);
    setRenamingId(null);
  };

  const cancelRename = (): void => {
    setRenamingId(null);
  };

  const handleDragStart = (e: React.DragEvent, id: string): void => {
    e.dataTransfer.setData(DRAG_MIME, id);
    e.dataTransfer.effectAllowed = "move";
    setDraggingId(id);
  };

  const handleDrop = (e: React.DragEvent, targetId: string): void => {
    e.preventDefault();
    setDraggingId(null);
    const draggedId = e.dataTransfer.getData(DRAG_MIME);
    if (draggedId !== "") reorderTerminal(workspaceId, draggedId, targetId);
  };

  if (terminals.length === 0) return null;

  return (
    <div className="flex shrink-0 items-center gap-0.5 border-b border-ink-muted/30 bg-surface px-1 py-0.5 text-xs">
      {terminals.map((term) => (
        <div
          key={term.id}
          role="tab"
          aria-selected={term.id === activeId}
          draggable={renamingId !== term.id}
          onDragStart={(e) => handleDragStart(e, term.id)}
          onDragEnd={() => setDraggingId(null)}
          onDragOver={(e) => {
            if (draggingId !== null && draggingId !== term.id) e.preventDefault();
          }}
          onDrop={(e) => handleDrop(e, term.id)}
          onClick={() => setActiveTerminal(workspaceId, term.id)}
          className={`group flex cursor-pointer items-center gap-1 rounded px-2 py-0.5 ${
            term.id === activeId
              ? "bg-surface-overlay text-ink-accent"
              : "text-ink-muted hover:bg-surface-overlay"
          } ${draggingId === term.id ? "opacity-50" : ""}`}
        >
          {renamingId === term.id ? (
            <input
              ref={inputRef}
              type="text"
              value={draft}
              maxLength={48}
              aria-label={t("terminal.rename")}
              onChange={(e) => setDraft(e.target.value)}
              onClick={(e) => e.stopPropagation()}
              onDoubleClick={(e) => e.stopPropagation()}
              onBlur={commitRename}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  commitRename();
                } else if (e.key === "Escape") {
                  e.preventDefault();
                  cancelRename();
                }
              }}
              className="w-24 rounded border border-ink-accent/60 bg-surface px-1 text-xs text-ink outline-none"
            />
          ) : (
            <span
              className="truncate"
              title={t("terminal.renameHint")}
              onDoubleClick={(e) => {
                e.stopPropagation();
                startRename(term);
              }}
            >
              {term.title}
            </span>
          )}
          <TerminalCwdBadge cwd={term.cwd} cwdSource={term.cwdSource} />
          <button
            type="button"
            aria-label={t("terminal.close")}
            onClick={(e) => { e.stopPropagation(); handleClose(term.id); }}
            className="rounded p-0.5 text-ink-muted/60 hover:bg-state-danger/20 hover:text-state-danger"
          >
            <Icon name="close" size={10} />
          </button>
        </div>
      ))}
      <button
        type="button"
        aria-label={t("terminal.new")}
        onClick={handleNew}
        className="flex items-center rounded px-1.5 py-0.5 text-ink-muted hover:bg-surface-overlay hover:text-ink-accent"
      >
        <Icon name="plus" size={10} />
      </button>
    </div>
  );
}