import type { ReactNode } from "react";
import { hashPaletteColor, type PaletteColor } from "../conversation/avatarPalette";

export interface SpeakerAsideProps {
  /** Display name under the avatar (truncated to the column width). */
  name: string;
  /** Deterministic palette seed (agent id); null → themed fallback. */
  seed?: string | null;
  /** Themed user marker: the user identity renders in the accent color. */
  isUser?: boolean;
}

/** User identity color — themed accent, the same fallback the retired
 *  RoleAvatar used for unknown agents. */
const USER_PALETTE: PaletteColor = { bg: "var(--nuomi-accent)", fg: "var(--nuomi-surface)" };

/** First character of a name for the avatar fallback (CJK-safe). */
function initial(name: string): string {
  return (name.trim()[0] ?? "?").toUpperCase();
}

/**
 * Vertical speaker column (SPEC 5.8.1 layout): round initial avatar on top,
 * speaker name below. User messages mirror it to the right edge via the
 * parent's flex-row-reverse; agent messages (and the pending-turn status
 * row) keep it on the left. Fixed 48px column so every row's text shares
 * one alignment axis.
 */
export function SpeakerAside({ name, seed = null, isUser = false }: SpeakerAsideProps): ReactNode {
  const palette = isUser || seed === null ? USER_PALETTE : hashPaletteColor(seed);
  return (
    <div className="flex w-12 shrink-0 flex-col items-center gap-0.5 pt-0.5">
      <div
        className="flex h-8 w-8 items-center justify-center rounded-full text-sm font-medium shadow-sm"
        title={name}
        aria-label={name}
        style={{ backgroundColor: palette.bg, color: palette.fg }}
      >
        {initial(name)}
      </div>
      <span className="w-full truncate text-center text-[10px] leading-tight text-ink-muted">
        {name}
      </span>
    </div>
  );
}
