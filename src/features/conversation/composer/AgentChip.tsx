import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { ConversationDto } from "../../../lib/ipc/client";
import { ipc } from "../../../lib/ipc/client";
import { agentDisplayName, isCliAgent } from "../../../lib/conversation/agentResolve";
import { AgentPickerPopover } from "./AgentPickerPopover";

export interface AgentChipProps {
  /** Current conversation (provides the agent ref). */
  conversation: ConversationDto | null;
  /** Session id for switching agent. */
  sessionId: string;
  /** Disable interaction when a run is in progress. */
  busy: boolean;
  /** Invalidate callback after agent switch. */
  onAgentChanged: () => void;
}

/**
 * Always-visible agent indicator in the composer's bottom-left corner.
 * Click opens the AgentPickerPopover for switching.
 */
export function AgentChip({ conversation, sessionId, busy, onAgentChanged }: AgentChipProps): ReactNode {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [switching, setSwitching] = useState(false);

  const agent = conversation?.agent ?? null;
  const name = agentDisplayName(agent);
  const isCli = isCliAgent(agent);

  const handleSelect = async (kind: string, id: string): Promise<void> => {
    setSwitching(true);
    try {
      await ipc.setConversationAgent(sessionId, { kind, id });
      onAgentChanged();
      setOpen(false);
    } finally {
      setSwitching(false);
    }
  };

  return (
    <div className="relative shrink-0">
      <button
        type="button"
        disabled={busy || switching}
        onClick={() => setOpen((v) => !v)}
        aria-label={t("composer.agentChipLabel")}
        className="flex items-center gap-1 rounded border border-ink-muted/30 px-2 py-1 text-xs text-ink-muted hover:bg-surface-overlay disabled:opacity-50"
      >
        <span aria-hidden="true">🤖</span>
        <span className="max-w-24 truncate">{switching ? "…" : name}</span>
        {isCli && agent && (
          <span className="rounded bg-ink-muted/20 px-1 text-[10px]">CLI</span>
        )}
      </button>
      {open && !busy && (
        <AgentPickerPopover
          sessionId={sessionId}
          onSelect={handleSelect}
          onClose={() => setOpen(false)}
        />
      )}
    </div>
  );
}
