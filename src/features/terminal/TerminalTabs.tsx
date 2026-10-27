/**
 * TerminalTabs (ADR-0024 Part B): multi-terminal tab strip, mirrors the
 * workbench FileTabs pattern. Each tab is a terminal instance in the
 * per-workspace bucket; [+] spawns a new one, × kills it.
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ipc } from "../../lib/ipc/client";
import { useUiStore, type TerminalInstance } from "../../lib/store/uiStore";
import { Icon } from "../../components/ui/Icon/Icon";
import { TerminalCwdBadge } from "./TerminalCwdBadge";
import { resolveTerminalCwd } from "./resolveTerminalCwd";

export function TerminalTabs({ workspaceId }: { workspaceId: string }): ReactNode {
  const { t } = useTranslation();
  const bucket = useUiStore((s) => s.terminalByWorkspace[workspaceId]);
  const addTerminal = useUiStore((s) => s.addTerminal);
  const removeTerminal = useUiStore((s) => s.removeTerminal);
  const setActiveTerminal = useUiStore((s) => s.setActiveTerminal);

  const terminals = bucket?.terminals ?? [];
  const activeId = bucket?.activeTerminalId ?? null;

  const handleNew = async (): Promise<void> => {
    const { cwd, source } = await resolveTerminalCwd();
    const id = `term-${Date.now()}`;
    const instance: TerminalInstance = {
      id,
      title: `Terminal ${terminals.length + 1}`,
      createdAt: Date.now(),
      cwd,
      cwdSource: source,
    };
    addTerminal(workspaceId, instance);
  };

  const handleClose = (terminalId: string): void => {
    void ipc.terminalKill(terminalId).catch(() => { /* already gone */ });
    removeTerminal(workspaceId, terminalId);
  };

  if (terminals.length === 0) return null;

  return (
    <div className="flex shrink-0 items-center gap-0.5 border-b border-ink-muted/30 bg-surface px-1 py-0.5 text-xs">
      {terminals.map((term) => (
        <div
          key={term.id}
          role="tab"
          aria-selected={term.id === activeId}
          onClick={() => setActiveTerminal(workspaceId, term.id)}
          className={`group flex cursor-pointer items-center gap-1 rounded px-2 py-0.5 ${
            term.id === activeId
              ? "bg-surface-overlay text-ink-accent"
              : "text-ink-muted hover:bg-surface-overlay"
          }`}
        >
          <span className="truncate">{term.title}</span>
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
