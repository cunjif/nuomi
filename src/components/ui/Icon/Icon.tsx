import type { SVGProps } from "react";
import { iconRegistry } from "./icons";

export type IconName = keyof typeof iconRegistry;

export interface IconProps extends Omit<SVGProps<SVGSVGElement>, "name"> {
  /** Which hand-drawn glyph to render (review §5). */
  name: IconName;
  /** Pixel size of the 24×24 viewBox (default 20). */
  size?: number;
  /** Hand-drawn stroke weight — 1.8 reads best at this scale (default 1.8). */
  strokeWidth?: number;
}

/**
 * Unified hand-drawn line-art icon (review §5.2.2). Every glyph is a 24×24
 * stroked path with round caps; colors inherit via `currentColor` so themes
 * and hover states just set `text-*`. Replaces the scattered Unicode glyphs
 * (⋯ × → ⚠) and ad-hoc inline SVGs across the app.
 */
export function Icon({ name, size = 20, strokeWidth = 1.8, ...rest }: IconProps) {
  const Svg = iconRegistry[name];
  return (
    <Svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...rest}
    />
  );
}
