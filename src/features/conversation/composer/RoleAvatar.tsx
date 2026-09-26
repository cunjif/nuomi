import type { ReactNode } from "react";
import type { ConversationDto } from "../../../lib/ipc/client";

export interface RoleAvatarProps {
  conversation: ConversationDto | null;
  /** "md" (default, 32px) or "sm" (24px, for compact bars). */
  size?: "sm" | "md";
}

/** Stable color from role id hash for avatar backgrounds. */
function avatarColor(id: string): string {
  const colors = [
    "#e76f51", "#2a9d8f", "#264653", "#e9c46a",
    "#457b9d", "#a8dadc", "#f4a261", "#6d597a",
  ];
  let hash = 0;
  for (const ch of id) hash = (hash * 31 + ch.charCodeAt(0)) | 0;
  return colors[Math.abs(hash) % colors.length]!;
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
  const color = agent ? avatarColor(agent.id) : "var(--nuomi-accent)";
  const sizeClass = size === "sm" ? "h-6 w-6 text-xs" : "h-8 w-8 text-sm";

  return (
    <div
      className={`flex ${sizeClass} shrink-0 items-center justify-center rounded-full font-medium text-surface shadow-sm`}
      title={name}
      aria-label={name}
      style={{ backgroundColor: color }}
    >
      {initial(name)}
    </div>
  );
}
