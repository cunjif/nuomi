import type { ReactNode } from "react";
import type { ConversationDto } from "../../../lib/ipc/client";
import { hashPaletteColor } from "../avatarPalette";

export interface RoleAvatarProps {
  conversation: ConversationDto | null;
  /** "md" (default, 32px) or "sm" (24px, for compact bars). */
  size?: "sm" | "md";
}

/** First letter (uppercased) of a name for the avatar fallback. */
function initial(name: string): string {
  return (name.trim()[0] ?? "?").toUpperCase();
}

/**
 * Circular Role Agent avatar — display-only. Shows the conversation's
 * first participant (single-chat semantics) as a colored circle with the
 * name's initial. Hover reveals the full name via a native tooltip.
 * ADR 0013: reads participantAgents[0] (no more conversation.agent).
 */
export function RoleAvatar({ conversation, size = "md" }: RoleAvatarProps): ReactNode {
  const agent = conversation?.participantAgents[0] ?? null;
  const name = agent?.name || agent?.id || "?";
  const palette = agent
    ? hashPaletteColor(agent.id)
    : { bg: "var(--nuomi-accent)", fg: "var(--nuomi-surface)" };
  const sizeClass = size === "sm" ? "h-6 w-6 text-xs" : "h-8 w-8 text-sm";

  return (
    <div
      className={`flex ${sizeClass} shrink-0 items-center justify-center rounded-full font-medium shadow-sm`}
      title={name}
      aria-label={name}
      style={{ backgroundColor: palette.bg, color: palette.fg }}
    >
      {initial(name)}
    </div>
  );
}
