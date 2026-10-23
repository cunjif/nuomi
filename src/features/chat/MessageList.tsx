import type { ReactNode } from "react";
import { useEffect } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useStickToBottom } from "../../components/ui/useStickToBottom";
import { Bubble } from "./Bubble";
import { AuthorizationBubble } from "./AuthorizationBubble";
import type { AuthorizationRequest } from "./useAuthorizationStream";
import type { ChatEntry } from "./useSessionStream";

/** Top padding of the scroll container. The Agent-detail `···` button is
 *  an embedded in-flow row above the scroller (ConversationView), not an
 *  overlay — no headroom is needed. */
const SCROLL_PT = "pt-1";

interface MessageListProps {
  entries: ChatEntry[];
  streamingId: string | null;
  /** Rendered INSIDE the scroll container after the messages — the agent
   * turn row (status pill / turn notice). Living in the scroll flow is
   * what keeps it above the composer: it can never overflow into the
   * composer's band the way an absolutely-positioned or h-full-sibling
   * pill could. */
  footer?: ReactNode;
  /** Swaps when the footer content changes (pill ↔ notice ↔ none) —
   * re-triggers stick-to-bottom so the new row scrolls into view. */
  footerKey?: string | null;
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
  /** Authorization requests rendered in the flow after the messages
   * (chat_rich_bind 5.4.1-3a — in-bubble authorization). */
  authorizations?: AuthorizationRequest[];
  /** Controlled clock for the timeout-reminder derivation (tests). */
  authorizationNow?: number;
  /** Refresh hook after a resolution lands (invalidates the stream). */
  onAuthorizationResolved?: () => void;
}

/** Plain list below the threshold; virtualized above it (AC11). */
const VIRTUALIZE_THRESHOLD = 100;

export function MessageList({ entries, streamingId, footer, footerKey, agentName, agentSeed, onQuote, onQuoteClick, onRollback, authorizations, authorizationNow, onAuthorizationResolved }: MessageListProps): ReactNode {
  const virtualized = entries.length > VIRTUALIZE_THRESHOLD;
  const { scrollRef, scrollToBottomIfStuck } = useStickToBottom({ rebindKey: virtualized });

  // Snap to the latest message whenever the list grows or the streaming
  // bubble updates — but only while the user is following the tail.
  const lastText = entries[entries.length - 1]?.text ?? "";
  useEffect(() => {
    scrollToBottomIfStuck();
  }, [entries.length, lastText, streamingId, virtualized, footerKey, scrollToBottomIfStuck]);

  const virtualizer = useVirtualizer({
    count: entries.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 56,
    overscan: 10,
    enabled: virtualized,
  });

  if (!virtualized) {
    return (
      <div ref={scrollRef} data-testid="message-scroll" className={`min-h-0 h-full overflow-y-auto ${SCROLL_PT} pb-2`}>
        {entries.map((entry) => (
          <div key={entry.id} data-entry-id={entry.id}>
            <Bubble entry={entry} streaming={entry.id === streamingId} agentName={agentName} agentSeed={agentSeed} onQuote={onQuote} onQuoteClick={onQuoteClick} onRollback={onRollback} />
          </div>
        ))}
        {(authorizations ?? []).map((request) => (
          <AuthorizationBubble
            key={request.authorizationId}
            request={request}
            now={authorizationNow}
            onResolve={onAuthorizationResolved}
          />
        ))}
        {footer}
      </div>
    );
  }
  return (
    <div ref={scrollRef} data-testid="message-scroll" className={`min-h-0 h-full overflow-y-auto ${SCROLL_PT} pb-2`}>
      <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
        {virtualizer.getVirtualItems().map((item) => (
          <div
            key={entries[item.index]?.id ?? item.key}
            data-entry-id={entries[item.index]?.id}
            style={{
              position: "absolute",
              top: 0,
              left: 0,
              width: "100%",
              transform: `translateY(${item.start}px)`,
            }}
          >
            <Bubble entry={entries[item.index] as ChatEntry} streaming={entries[item.index]?.id === streamingId} agentName={agentName} agentSeed={agentSeed} onQuote={onQuote} onQuoteClick={onQuoteClick} onRollback={onRollback} />
          </div>
        ))}
      </div>
      {footer}
    </div>
  );
}
