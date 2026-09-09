import type { HTMLAttributes, ReactNode } from "react";

/**
 * Hand-drawn badge (review §6.2 / §11.2). Dashed ink border + scribble face;
 * tones map to semantic tokens so it stays legible across all four themes.
 */
export type BadgeTone = "neutral" | "accent" | "ok" | "warn" | "danger";

const TONES: Record<BadgeTone, string> = {
  neutral: "border-ink-muted text-ink-muted",
  accent: "border-ink-accent text-ink-accent",
  ok: "border-state-ok text-state-ok",
  warn: "border-state-warn text-state-warn",
  danger: "border-danger text-danger",
};

export interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  tone?: BadgeTone;
}

export function Badge({ tone = "neutral", className = "", ...rest }: BadgeProps): ReactNode {
  return (
    <span
      className={`inline-flex items-center gap-1 rounded-full border border-dashed px-2 py-0.5 font-scribble text-[11px] leading-none ${TONES[tone]} ${className}`}
      {...rest}
    />
  );
}
