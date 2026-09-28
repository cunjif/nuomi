/**
 * Deterministic workspace badge color: hashes the workspace root path to a
 * fixed 8-color palette. Same path always yields the same color so a workspace
 * is visually stable across the conversations list and the new-conversation
 * wizard. The algorithm mirrors `avatarColor` in NewConversationDialog for
 * visual consistency (31× hash → 8 colors).
 */
const WORKSPACE_BADGE_COLORS = [
  "#e76f51", "#2a9d8f", "#264653", "#e9c46a",
  "#457b9d", "#a8dadc", "#f4a261", "#6d597a",
] as const;

export function workspaceBadgeColor(rootPath: string): string {
  let hash = 0;
  for (const ch of rootPath) hash = (hash * 31 + ch.charCodeAt(0)) | 0;
  return WORKSPACE_BADGE_COLORS[Math.abs(hash) % WORKSPACE_BADGE_COLORS.length]!;
}
