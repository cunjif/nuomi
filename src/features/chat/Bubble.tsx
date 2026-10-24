import type { MouseEvent, ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ToolCard } from "./ToolCard";
import { QuoteBlock } from "./QuoteBlock";
import { SpeakerAside } from "./SpeakerAside";
import { MarkdownView } from "./MarkdownView";
import { HtmlPreviewEntry } from "./HtmlPreviewEntry";
import { SourceEvidence } from "./SourceEvidence";
import { BubbleToolbar, QUOTE_ICON, ROLLBACK_ICON, makeCopyAction, type BubbleToolbarAction } from "./BubbleToolbar";
import { seqFromId, type ChatEntry } from "./useSessionStream";

interface BubbleProps {
  entry: ChatEntry;
  /** True while tokens are still streaming into this bubble. */
  streaming?: boolean;
  /** Bound Role Agent display name (conversation participantAgents[0]). */
  agentName?: string | null;
  /** Palette seed for the agent avatar (participantAgents[0].id). */
  agentSeed?: string | null;
  /** Called when the user clicks the reply button on a message. */
  onQuote?: (seq: number) => void;
  /** Called when the user clicks a quote preview inside a message. */
  onQuoteClick?: (seq: number) => void;
  /** Called when the user clicks "rollback to here" on a user message. */
  onRollback?: (seq: number) => void;
  /** Whether the click-to-open action toolbar is open for this bubble. */
  toolbarOpen?: boolean;
  /** Clicking the bubble body toggles the toolbar (WeChat-style). */
  onToolbarToggle?: () => void;
  /** Closes the toolbar after one of its actions has run. */
  onToolbarClose?: () => void;
  /** True when the previous entry spoke as the OTHER side (user ↔ agent):
   *  adds breathing room at role switches (WeChat-style grouping). */
  roleGap?: boolean;
}

/** User bubbles right (mirrored aside), assistant left; tool traffic
 *  renders as folded cards (no speaker column — they are traffic, not
 *  utterances). Assistant bubbles carry a hairline border; clicking a
 *  bubble body pops a WeChat-style action bar (quote / rollback / copy)
 *  above it. */
export function Bubble({ entry, streaming = false, agentName = null, agentSeed = null, onQuote, onQuoteClick, onRollback, toolbarOpen = false, onToolbarToggle, onToolbarClose, roleGap = false }: BubbleProps): ReactNode {
  const { t } = useTranslation();
  if (entry.kind === "tool_call") {
    return <ToolCard label={t("chat.toolCall")} tool={entry.tool ?? ""} body={entry.argsJson ?? "{}"} />;
  }
  if (entry.kind === "tool_result") {
    return <ToolCard label={t("chat.toolResult")} tool="" body={entry.content ?? ""} />;
  }
  const isUser = entry.role === "user";
  const speaker = entry.roleName ?? agentName ?? entry.role ?? "assistant";
  const seq = seqFromId(entry.id);
  const canReply = onQuote !== undefined && seq !== null;
  const canRollback = onRollback !== undefined && seq !== null && isUser;
  const closeToolbar = onToolbarClose ?? (() => {});
  const copyAction = makeCopyAction(entry.text, t, closeToolbar);

  const actions: BubbleToolbarAction[] = [];
  if (canReply) {
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
  if (canRollback) {
    actions.push({
      key: "rollback",
      label: t("chat.rollbackToHere"),
      icon: ROLLBACK_ICON,
      onClick: () => {
        closeToolbar();
        onRollback!(seq!);
      },
    });
  }
  if (copyAction !== null) actions.push(copyAction);
  const toolbarAvailable = actions.length > 0 && onToolbarToggle !== undefined;

  const handleBubbleClick = (e: MouseEvent<HTMLDivElement>): void => {
    if (!toolbarAvailable) return;
    // Clicks on interactive content (links, quote blocks) are theirs, not ours.
    if ((e.target as HTMLElement).closest("a, button") !== null) return;
    // A click that ends an in-bubble text selection must not pop the toolbar.
    if ((window.getSelection()?.toString() ?? "") !== "") return;
    onToolbarToggle!();
  };

  return (
    <div className={`flex gap-2 px-3 py-1 ${roleGap ? "mt-2" : ""} ${isUser ? "flex-row-reverse" : ""}`}>
      <SpeakerAside name={isUser ? t("chat.you") : speaker} seed={agentSeed} isUser={isUser} />
      <div
        onClick={handleBubbleClick}
        className={`relative max-w-[80%] rounded-lg px-3 py-2 text-sm ${
          toolbarAvailable ? "cursor-pointer" : ""
        } ${
          isUser
            ? "bg-ink-accent/20 text-ink"
            : "border border-ink-muted/40 bg-surface-raised text-ink"
        }`}
      >
        {toolbarOpen && (
          <BubbleToolbar actions={actions} align={isUser ? "right" : "left"} />
        )}
        {entry.quotedEventSeq !== undefined && entry.quotedExcerpt !== undefined && (
          <QuoteBlock
            excerpt={entry.quotedExcerpt}
            onClick={onQuoteClick !== undefined ? () => onQuoteClick!(entry.quotedEventSeq!) : undefined}
          />
        )}
        <MarkdownView text={entry.text || t("chat.emptyResponse")} streaming={streaming} />
        {!isUser && <HtmlPreviewEntry text={entry.text ?? ""} />}
        {!isUser && entry.sources !== undefined && <SourceEvidence sources={entry.sources} />}
      </div>
    </div>
  );
}
