import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { AgentRefDto } from "../../../lib/ipc/client";
import { AddAgentPopover } from "./AddAgentPopover";
import { hashPaletteColor } from "../avatarPalette";

export interface AgentAvatarListProps {
  agents: AgentRefDto[];
  sessionId: string;
  onAgentClick: (agent: AgentRefDto) => void;
  onAgentAdded: () => void;
}

/** First letter (uppercased) of a name for the avatar fallback. */
function initial(name: string): string {
  return (name.trim()[0] ?? "?").toUpperCase();
}

/**
 * Horizontal list of circular agent avatars with a trailing "+" button
 * that opens the add-agent popover. Clicking an avatar triggers the
 * BottomSheet via `onAgentClick`.
 */
export function AgentAvatarList({
  agents,
  sessionId,
  onAgentClick,
  onAgentAdded,
}: AgentAvatarListProps): ReactNode {
  const { t } = useTranslation();
  const [showAdd, setShowAdd] = useState(false);

  const excludeKeys = new Set(agents.map((a) => `${a.kind}:${a.id}`));

  return (
    <div className="flex items-center gap-2 p-3">
      {agents.map((agent) => {
        const palette = hashPaletteColor(agent.id);
        return (
          <button
            key={`${agent.kind}:${agent.id}`}
            type="button"
            onClick={() => onAgentClick(agent)}
            className="group relative shrink-0"
            title={agent.name}
            aria-label={agent.name}
          >
            <span
              className="flex h-9 w-9 items-center justify-center rounded-full text-sm font-medium"
              style={{ backgroundColor: palette.bg, color: palette.fg }}
            >
              {initial(agent.name)}
            </span>
          </button>
        );
      })}
      <div className="relative shrink-0">
        <button
          type="button"
          onClick={() => setShowAdd((v) => !v)}
          className="flex h-9 w-9 items-center justify-center rounded-full border border-dashed border-ink-muted text-ink-muted hover:border-ink-accent hover:text-ink-accent"
          aria-label={t("conversation.agentAvatar.add")}
        >
          +
        </button>
        {showAdd && (
          <AddAgentPopover
            sessionId={sessionId}
            excludeAgentKeys={excludeKeys}
            onAdded={() => { setShowAdd(false); onAgentAdded(); }}
            onClose={() => setShowAdd(false)}
          />
        )}
      </div>
    </div>
  );
}
