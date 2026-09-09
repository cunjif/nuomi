import type { ComponentPropsWithoutRef, ReactNode, SelectHTMLAttributes, TextareaHTMLAttributes } from "react";

/**
 * Unified hand-drawn form field (review §6.2). Replaces the five near-identical
 * `const field = "..."` strings scattered through the settings forms with one
 * token-aware atom: dashed ink border (`.sketch-input`), wobbly radius, ink text
 * with muted placeholder, and an accent focus ring. Per-form typography (text-xs
 * vs text-sm) and surface tint are passed through `className`.
 */
/**
 * Shared hand-drawn field class string. The five settings forms previously
 * each inlined a near-identical `const field = "..."`; they now import this one
 * source of truth and append per-form surface/typography (e.g. `bg-surface
 * text-sm`). Kept free of bg/width/size so callers control those locally.
 */
export const fieldClass =
  "sketch-input px-2 py-1 text-ink placeholder:text-ink-muted " +
  "focus:outline-none focus-visible:ring-2 focus-visible:ring-ink-accent/70 disabled:opacity-50";

const BASE = `${fieldClass} w-full bg-surface-overlay`;

type InputProps = ComponentPropsWithoutRef<"input">;
type TextareaProps = TextareaHTMLAttributes<HTMLTextAreaElement>;
type SelectProps = SelectHTMLAttributes<HTMLSelectElement>;

export function Field({ className = "", ...rest }: InputProps): ReactNode {
  return <input className={`${BASE} ${className}`} {...rest} />;
}

export function TextareaField({ className = "", ...rest }: TextareaProps): ReactNode {
  return <textarea className={`${BASE} ${className}`} {...rest} />;
}

export function SelectField({ className = "", children, ...rest }: SelectProps): ReactNode {
  return (
    <select className={`${BASE} ${className}`} {...rest}>
      {children}
    </select>
  );
}
