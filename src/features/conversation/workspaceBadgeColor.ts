/**
 * Deterministic workspace badge color: hashes the workspace id to a fixed
 * 8-color palette. Same workspace always yields the same color everywhere
 * (conversations list, chat tabs, new-conversation wizard), and the color
 * survives directory set changes because the hash source is the stable
 * workspace id, not a path (ADR-0021 two-level model). The algorithm mirrors
 * `avatarColor` in NewConversationDialog for visual consistency (31× hash →
 * 8 colors).
 */
const WORKSPACE_BADGE_COLORS = [
  "#e76f51", "#2a9d8f", "#264653", "#e9c46a",
  "#457b9d", "#a8dadc", "#f4a261", "#6d597a",
] as const;

export function workspaceBadgeColor(workspaceId: string): string {
  let hash = 0;
  for (const ch of workspaceId) hash = (hash * 31 + ch.charCodeAt(0)) | 0;
  return WORKSPACE_BADGE_COLORS[Math.abs(hash) % WORKSPACE_BADGE_COLORS.length]!;
}
