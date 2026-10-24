import type { MouseEvent, ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { QuoteBlock } from "../../chat/QuoteBlock";
import { MarkdownView } from "../../chat/MarkdownView";
import { HtmlPreviewEntry } from "../../chat/HtmlPreviewEntry";
import { SourceEvidence } from "../../chat/SourceEvidence";
import { BubbleToolbar, QUOTE_ICON, makeCopyAction, type BubbleToolbarAction } from "../../chat/BubbleToolbar";
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
   *  edge (spec 5.2.1-2); omitted/empty renders nothing. */
  sources?: SourceEntryDto[];
  /** Event seq of this message; enables the quote action in the toolbar. */
  seq?: number | null;
  /** Called when the quote action is picked from the toolbar. */
  onQuote?: (seq: number) => void;
  /** Whether the click-to-open action toolbar is open for this bubble. */
  toolbarOpen?: boolean;
  /** Clicking the bubble body toggles the toolbar (WeChat-style). */
  onToolbarToggle?: () => void;
  /** Closes the toolbar after one of its actions has run. */
  onToolbarClose?: () => void;
  /** True when the previous message spoke as the OTHER side (user ↔
   *  agent): adds breathing room at role switches. */
  roleGap?: boolean;
}

/** RoleAgent message bubble: role color bar + role-name tag + a rounded
 *  BubbleBody (same surface/radius/padding/border as the single-chat
 *  assistant bubble) wrapping Markdown-rendered content. Handoff marker
 *  and quote blocks keep their pre-existing behavior (spec 5.1.1-1a).
 *  Clicking the body pops a WeChat-style toolbar (quote / copy). */
export function SpeakerBubble({ roleName, roleColor, content, isHandoff, quotedExcerpt, onQuoteClick, sources, seq, onQuote, toolbarOpen = false, onToolbarToggle, onToolbarClose, roleGap = false }: SpeakerBubbleProps): ReactNode {
  const { t } = useTranslation();
  const closeToolbar = onToolbarClose ?? (() => {});
  const copyAction = makeCopyAction(content, t, closeToolbar);
  const canQuote = onQuote !== undefined && seq !== null && seq !== undefined;

  const actions: BubbleToolbarAction[] = [];
  if (canQuote) {
    actions.push({
      key: "quote",
      label: t("chat.quote"),
      icon: QUOTE_ICON,
      onClick: () => {
        closeToolbar();
        onQuote!(seq!);
      },
    });
  }
  if (copyAction !== null) actions.push(copyAction);
  const toolbarAvailable = actions.length > 0 && onToolbarToggle !== undefined;

  const handleBodyClick = (e: MouseEvent<HTMLDivElement>): void => {
    if (!toolbarAvailable) return;
    if ((e.target as HTMLElement).closest("a, button") !== null) return;
    if ((window.getSelection()?.toString() ?? "") !== "") return;
    onToolbarToggle!();
  };

  return (
    <div className={`flex gap-2 py-1 ${roleGap ? "mt-2" : ""}`}>
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
        <div
          data-testid="speaker-bubble-body"
          onClick={handleBodyClick}
          className={`relative mt-0.5 max-w-full rounded-lg border border-ink-muted/40 bg-surface-raised px-3 py-2 text-sm text-ink ${
            toolbarAvailable ? "cursor-pointer" : ""
          }`}
        >
          {toolbarOpen && <BubbleToolbar actions={actions} />}
          <MarkdownView text={content} />
          <HtmlPreviewEntry text={content} />
          {sources !== undefined && sources.length > 0 && <SourceEvidence sources={sources} />}
        </div>
      </div>
    </div>
  );
}
