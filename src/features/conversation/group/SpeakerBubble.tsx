import type { ReactNode } from "react";
import { QuoteBlock } from "../../chat/QuoteBlock";
import { MarkdownView } from "../../chat/MarkdownView";
import { HtmlPreviewEntry } from "../../chat/HtmlPreviewEntry";
import { SourceEvidence } from "../../chat/SourceEvidence";
import type { PaletteColor } from "../avatarPalette";
import type { SourceEntryDto } from "../../../lib/events/types";

export interface SpeakerBubbleProps {
  roleName: string;
  roleColor: PaletteColor;
  content: string;
  isHandoff?: boolean;
  quotedExcerpt?: string;
  onQuoteClick?: () => void;
  /** Source evidence entries rendered as a chip row at the bubble's bottom
   * edge (spec 5.2.1-2); omitted/empty renders nothing. */
  sources?: SourceEntryDto[];
}

/** RoleAgent message bubble: role color bar + role-name tag + a rounded
 * BubbleBody (same surface/radius/padding as the single-chat assistant
 * bubble) wrapping Markdown-rendered content. Handoff marker and quote
 * blocks keep their pre-existing behavior (spec 5.1.1-1a). */
export function SpeakerBubble({ roleName, roleColor, content, isHandoff, quotedExcerpt, onQuoteClick, sources }: SpeakerBubbleProps): ReactNode {
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
        <div data-testid="speaker-bubble-body" className="mt-0.5 max-w-full rounded-lg bg-surface-raised px-3 py-2 text-sm text-ink">
          <MarkdownView text={content} />
          <HtmlPreviewEntry text={content} />
          {sources !== undefined && sources.length > 0 && <SourceEvidence sources={sources} />}
        </div>
      </div>
    </div>
  );
}
