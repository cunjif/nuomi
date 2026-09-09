import type { ButtonHTMLAttributes, ReactNode } from "react";

/**
 * Hand-drawn button (review §6.2 / §7.2). Solid = ink fill; outline = dashed
 * ink border that firms up + casts a sketch shadow on hover; ghost = bare text.
 * Wobbly radius and hard-edged offset shadow are the signature "sketch" look.
 */
export type ButtonVariant = "solid" | "outline" | "ghost";
export type ButtonSize = "sm" | "md" | "icon";

const VARIANTS: Record<ButtonVariant, string> = {
  solid: "border-2 border-ink bg-ink text-surface shadow-sketch-sm hover:shadow-sketch-md active:translate-x-px active:translate-y-px",
  outline:
    "border border-dashed border-ink-muted bg-surface text-ink hover:border-ink hover:shadow-sketch-sm active:translate-x-px active:translate-y-px",
  ghost: "border border-transparent text-ink-muted hover:text-ink hover:bg-surface-raised",
};

const SIZES: Record<ButtonSize, string> = {
  sm: "px-2 py-0.5 text-xs",
  md: "px-3 py-1 text-sm",
  icon: "p-1.5",
};

const WOBBLE = "rounded-[12px_255px_15px_225px/225px_15px_255px_12px]";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
}

export function Button({
  variant = "outline",
  size = "md",
  className = "",
  type = "button",
  ...rest
}: ButtonProps): ReactNode {
  return (
    <button
      type={type}
      className={`inline-flex items-center justify-center gap-1.5 font-note-hand transition-[box-shadow,transform,border-color] disabled:cursor-not-allowed disabled:opacity-50 ${WOBBLE} ${VARIANTS[variant]} ${SIZES[size]} ${className}`}
      {...rest}
    />
  );
}
