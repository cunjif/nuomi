import type { ReactNode } from "react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AgentRefDto, ConversationDto } from "../../../lib/ipc/client";
import { ipc } from "../../../lib/ipc/client";
import { useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../../i18n";
import { toast } from "../../../lib/store/toastStore";
import { AgentAvatarList } from "./AgentAvatarList";
import { ConversationInfoPanel } from "./ConversationInfoPanel";
import { AgentBottomSheet } from "./AgentBottomSheet";

export interface AgentSidebarProps {
  conversation: ConversationDto;
  sessionId: string;
  open: boolean;
  onClose: () => void;
}

/**
 * Right-side sidebar showing agent participants (upper) and conversation
 * info (lower). Slides in from the right with a 200ms CSS transition.
 * Clicking an avatar opens a BottomSheet overlaying the lower section.
 * Closing the sidebar also closes the BottomSheet (state machine: no
 * orphaned floating panels).
 */
export function AgentSidebar({
  conversation,
  sessionId,
  open,
  onClose,
}: AgentSidebarProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [selectedAgent, setSelectedAgent] = useState<AgentRefDto | null>(null);

  // State machine: closing the sidebar also dismisses the BottomSheet.
  useEffect(() => {
    if (!open) setSelectedAgent(null);
  }, [open]);

  // Click outside to close (backdrop).
  const onBackdropClick = (): void => {
    onClose();
  };

  // ADR 0013: "Remove member" only in group chat (participantAgents.length > 1).
  const canRemove = conversation.participantAgents.length > 1;

  const handleRemove = async (): Promise<void> => {
    if (!selectedAgent) return;
    try {
      await ipc.removeConversationAgent(sessionId, { kind: selectedAgent.kind, id: selectedAgent.id });
      void qc.invalidateQueries({ queryKey: ["conversation", sessionId] });
      setSelectedAgent(null);
    } catch (e) {
      // P1-5: surface the error instead of silently swallowing it.
      toast.error(`${t("conversation.removeMemberFailed")}: ${describeError(e)}`);
    }
  };

  return (
    <>
      {open && (
        <div
          className="fixed inset-0 z-10 bg-ink/10"
          onClick={onBackdropClick}
          aria-hidden="true"
        />
      )}
      <aside
        className={`fixed right-0 top-0 z-20 flex h-full w-80 flex-col border-l border-ink-muted/40 bg-surface shadow-lg transition-transform duration-200 ease-out ${
          open ? "translate-x-0" : "translate-x-full"
        }`}
        role="complementary"
        aria-label={t("conversation.agentSidebar.title")}
        aria-hidden={!open}
      >
        {/* Header */}
        <div className="flex shrink-0 items-center justify-between border-b border-ink-muted/30 px-3 py-2">
          <span className="text-sm font-medium text-ink">
            {t("conversation.agentSidebar.title")}
          </span>
          <button
            type="button"
            onClick={onClose}
            className="text-ink-muted hover:text-ink"
            aria-label={t("common.close")}
          >
            ✕
          </button>
        </div>

        {/* Upper: avatar list */}
        <div className="shrink-0 border-b border-ink-muted/30">
          <AgentAvatarList
            agents={conversation.participantAgents}
            sessionId={sessionId}
            onAgentClick={(agent) => setSelectedAgent(agent)}
            onAgentAdded={() => { /* conversation query invalidated by AddAgentPopover */ }}
          />
        </div>

        {/* Lower: conversation info panel (scrollable) */}
        <div className="min-h-0 flex-1 overflow-y-auto">
          <ConversationInfoPanel conversation={conversation} sessionId={sessionId} />
        </div>

        {/* BottomSheet overlaying the lower section */}
        {selectedAgent && open && (
          <AgentBottomSheet
            agentKind={selectedAgent.kind}
            agentId={selectedAgent.id}
            onClose={() => setSelectedAgent(null)}
            canRemove={canRemove}
            onRemove={handleRemove}
          />
        )}
      </aside>
    </>
  );
}
