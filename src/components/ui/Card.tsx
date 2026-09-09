import type { HTMLAttributes, ReactNode } from "react";

/**
 * Hand-drawn panel (review §6.2 / §11.1). Dashed ink outline + wobbly radius,
 * lifted onto the raised surface. The `.sketch-card` class lives in global.css
 * so plain markup gets the same look without importing this atom.
 */
export interface CardProps extends HTMLAttributes<HTMLDivElement> {
  /** Remove default inner padding (e.g. for edge-to-edge media). */
  flush?: boolean;
}

export function Card({ flush = false, className = "", ...rest }: CardProps): ReactNode {
  return (
    <div
      className={`sketch-card bg-surface-raised ${flush ? "" : "p-3"} ${className}`}
      {...rest}
    />
  );
}
