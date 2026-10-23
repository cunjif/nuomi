import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ToolCard } from "./ToolCard";
import { QuoteBlock } from "./QuoteBlock";
import { SpeakerAside } from "./SpeakerAside";
import type { ChatEntry } from "./useSessionStream";

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
}

/** Extract the event seq from a history entry id (`m42` → 42). */
function seqFromId(id: string): number | null {
  const m = /^m(\d+)$/.exec(id);
  return m ? Number(m[1]) : null;
}

/** User bubbles right (mirrored aside), assistant left; tool traffic
 * renders as folded cards (no speaker column — they are traffic, not
 * utterances). */
export function Bubble({ entry, streaming = false, agentName = null, agentSeed = null, onQuote, onQuoteClick, onRollback }: BubbleProps): ReactNode {
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
  return (
    <div className={`group flex gap-2 px-3 py-1 ${isUser ? "flex-row-reverse" : ""}`}>
      <SpeakerAside name={isUser ? t("chat.you") : speaker} seed={agentSeed} isUser={isUser} />
      <div
        className={`relative max-w-[80%] rounded-lg px-3 py-2 text-sm ${
          isUser ? "bg-ink-accent/20 text-ink" : "bg-surface-raised text-ink"
        }`}
      >
        {canReply && (
          <button
            type="button"
            aria-label={t("chat.reply")}
            onClick={() => onQuote!(seq!)}
            className="absolute -top-2 right-0 flex h-5 w-5 items-center justify-center rounded-full bg-surface-raised text-ink-muted opacity-0 shadow-sm transition-opacity hover:text-ink group-hover:opacity-100"
          >
            <svg viewBox="0 0 16 16" className="h-3 w-3 fill-current">
              <path d="M6 2L2 6l4 4V8h3c2.2 0 4 1.8 4 4v2h-2v-2c0-1.1-.9-2-2-2H6V2z" />
            </svg>
          </button>
        )}
        {canRollback && (
          <button
            type="button"
            aria-label={t("chat.rollbackToHere")}
            onClick={() => onRollback!(seq!)}
            className="absolute -top-2 left-0 flex h-5 w-5 items-center justify-center rounded-full bg-surface-raised text-ink-muted opacity-0 shadow-sm transition-opacity hover:text-ink group-hover:opacity-100"
          >
            <svg viewBox="0 0 16 16" className="h-3 w-3 fill-current">
              <path d="M8 2a6 6 0 1 0 0 12A6 6 0 0 0 8 2zm2.5 6.5H6.7l1.4-1.4-1-1L4 8l3.1 3.1 1-1-1.4-1.4H10.5a.5.5 0 0 1 .5.5v.3a.5.5 0 0 1-.5.5z" />
            </svg>
          </button>
        )}
        {entry.quotedEventSeq !== undefined && entry.quotedExcerpt !== undefined && (
          <QuoteBlock
            excerpt={entry.quotedExcerpt}
            onClick={onQuoteClick !== undefined ? () => onQuoteClick!(entry.quotedEventSeq!) : undefined}
          />
        )}
        <p className="whitespace-pre-wrap break-words">
          {entry.text || t("chat.emptyResponse")}
          {streaming && (
            <span aria-hidden="true" className="ml-0.5 inline-block h-3 w-1.5 animate-pulse bg-ink-muted align-middle" />
          )}
        </p>
      </div>
    </div>
  );
}
