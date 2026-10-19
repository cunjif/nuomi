import type { ReactNode } from "react";
import { QuoteBlock } from "../../chat/QuoteBlock";
import type { PaletteColor } from "../avatarPalette";

export interface SpeakerBubbleProps {
  roleName: string;
  roleColor: PaletteColor;
  content: string;
  isHandoff?: boolean;
  quotedExcerpt?: string;
  onQuoteClick?: () => void;
}

/** Speaker message bubble with colored role indicator. */
export function SpeakerBubble({ roleName, roleColor, content, isHandoff, quotedExcerpt, onQuoteClick }: SpeakerBubbleProps): ReactNode {
  return (
    <div className="flex gap-2 py-1">
      <div className="w-1 shrink-0 rounded-full" style={{ backgroundColor: roleColor.bg }} />
      <div className="min-w-0 flex-1">
        <span
          className="inline-block rounded px-1.5 py-0.5 text-xs font-medium"
          style={{ backgroundColor: `${roleColor.bg}20`, color: "var(--nuomi-ink)" }}
        >
          {roleName}
          {isHandoff && " →"}
        </span>
        {quotedExcerpt !== undefined && <QuoteBlock excerpt={quotedExcerpt} onClick={onQuoteClick} />}
        <p className="mt-0.5 text-sm text-ink">{content}</p>
      </div>
    </div>
  );
}
