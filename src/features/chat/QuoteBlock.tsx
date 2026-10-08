import type { ReactNode } from "react";

interface QuoteBlockProps {
  excerpt: string;
  onClick?: () => void;
}

/** Compact quote preview rendered above a message bubble's text. */
export function QuoteBlock({ excerpt, onClick }: QuoteBlockProps): ReactNode {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={onClick === undefined}
      className="mb-1 block w-full cursor-pointer border-l-2 border-ink-muted/50 bg-surface-overlay/50 px-2 py-1 text-left text-xs text-ink-muted line-clamp-2 hover:bg-surface-overlay disabled:cursor-default"
    >
      {excerpt}
    </button>
  );
}
