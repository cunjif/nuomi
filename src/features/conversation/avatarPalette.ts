/**
 * Shared 8-color hash palette for agent avatars and workspace badges.
 * Every consumer (RoleAvatar, AgentAvatarList, NewConversationDialog,
 * GroupConversationView, workspaceBadgeColor) used to carry its own copy of
 * the same palette + 31× hash — this module is the single home now.
 *
 * Each entry pairs its background with a foreground that clears WCAG 4.5:1
 * for the avatar initial: light swatches (#e9c46a/#a8dadc/#f4a261, and the
 * mid-tone #e76f51/#2a9d8f which only reach ~3:1 with white) take black;
 * dark swatches (#264653/#457b9d/#6d597a) take white. Avatar backgrounds
 * are fixed decorative colors — they do not vary per theme, so neither do
 * their foregrounds.
 */
export interface PaletteColor {
  readonly bg: string;
  readonly fg: string;
}

const AVATAR_PALETTE: readonly PaletteColor[] = [
  { bg: "#e76f51", fg: "#000000" },
  { bg: "#2a9d8f", fg: "#000000" },
  { bg: "#264653", fg: "#ffffff" },
  { bg: "#e9c46a", fg: "#000000" },
  { bg: "#457b9d", fg: "#ffffff" },
  { bg: "#a8dadc", fg: "#000000" },
  { bg: "#f4a261", fg: "#000000" },
  { bg: "#6d597a", fg: "#ffffff" },
];

/** Deterministic 31× string hash → palette entry (same seed, same color). */
export function hashPaletteColor(seed: string): PaletteColor {
  let hash = 0;
  for (const ch of seed) hash = (hash * 31 + ch.charCodeAt(0)) | 0;
  return AVATAR_PALETTE[Math.abs(hash) % AVATAR_PALETTE.length]!;
}
