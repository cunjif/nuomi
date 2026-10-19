/**
 * Deterministic workspace badge color: hashes the workspace id into the
 * shared 8-color avatar palette (avatarPalette.ts — single home for the
 * 31× hash and the swatches). Same workspace always yields the same color
 * everywhere (conversations list, chat tabs, new-conversation wizard), and
 * the color survives directory set changes because the hash source is the
 * stable workspace id, not a path (ADR-0021 two-level model). Badges are
 * color dots / tinted chips without text of their own, so only the
 * background color is needed here.
 */
import { hashPaletteColor } from "./avatarPalette";

export function workspaceBadgeColor(workspaceId: string): string {
  return hashPaletteColor(workspaceId).bg;
}
