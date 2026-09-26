import { useState, useRef, useMemo, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ipc } from "../../lib/ipc/client";
import { useUiStore } from "../../lib/store/uiStore";
import { ChatTab } from "./ChatTab";
import { ConnectionStatusBadge } from "./ConnectionStatusBadge";
import { NewConversationDialog } from "../conversation/new/NewConversationDialog";
import type { ConversationKind } from "../../lib/conversation/kinds";

/** Virtualization threshold — below this, render all tabs. */
const TAB_VIRTUALIZE_THRESHOLD = 30;
/** Assumed average tab width (px) for virtualization windowing. */
const ASSUMED_TAB_WIDTH = 120;

/**
 * Multi-chat tab bar rendered in AreaNav row 2. Three-segment layout:
 * leading indent + horizontal-scroll tab list + fixed-right connection badge.
 * Compact: 12px icon + xs title + px-2 py-0.5 + gap-1.
 */
export function ChatTabBar(): ReactNode {
  const { t } = useTranslation();
  const openSessionIds = useUiStore((s) => s.openSessionIds);
  const selectedSessionId = useUiStore((s) => s.selectedSessionId);
  const activateChatTab = useUiStore((s) => s.activateChatTab);
  const closeChatTab = useUiStore((s) => s.closeChatTab);
  const [dialogOpen, setDialogOpen] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  const { data: conversations } = useQuery({
    queryKey: ["conversations"],
    queryFn: () => ipc.listConversations(null),
    staleTime: 10_000,
  });

  const convMap = useMemo(() => {
    const m = new Map((conversations ?? []).map((c) => [c.id, c]));
    return m;
  }, [conversations]);

  // Virtualization: when tab count exceeds threshold, window based on scrollLeft.
  const [scrollLeft, setScrollLeft] = useState(0);
  const viewportWidth = scrollRef.current?.clientWidth ?? 0;
  const virtualize = openSessionIds.length > TAB_VIRTUALIZE_THRESHOLD;
  const startIdx = virtualize
    ? Math.max(0, Math.floor(scrollLeft / ASSUMED_TAB_WIDTH) - 2)
    : 0;
  const endIdx = virtualize
    ? Math.min(
        openSessionIds.length,
        startIdx + Math.ceil((viewportWidth + 4 * ASSUMED_TAB_WIDTH) / ASSUMED_TAB_WIDTH),
      )
    : openSessionIds.length;
  const visibleIds = openSessionIds.slice(startIdx, endIdx);

  return (
    <div className="flex w-full items-center gap-1">
      {/* Tab scroll region (leading indent via pl-2 above). */}
      <div
        ref={scrollRef}
        onScroll={(e) => setScrollLeft((e.target as HTMLDivElement).scrollLeft)}
        className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto"
        role="tablist"
        aria-label={t("nav.chatTabsLabel")}
      >
        {visibleIds.map((id) => {
          const conv = convMap.get(id);
          return (
            <ChatTab
              key={id}
              sessionId={id}
              kind={conv?.kind ?? "chat"}
              title={conv?.title ?? id}
              active={selectedSessionId === id}
              onActivate={activateChatTab}
              onClose={closeChatTab}
            />
          );
        })}
        {/* "+" new conversation button. */}
        <button
          type="button"
          onClick={() => setDialogOpen(true)}
          className="flex shrink-0 items-center justify-center rounded px-2 py-0.5 text-xs text-ink-muted hover:bg-surface-overlay hover:text-ink-accent"
          aria-label={t("conversation.new")}
          title={t("conversation.new")}
        >
          +
        </button>
      </div>
      {/* Connection status fixed right (does not scroll). */}
      <div className="flex shrink-0 items-center">
        <ConnectionStatusBadge />
      </div>
      {dialogOpen && (
        <NewConversationDialog
          kind={"chat" as ConversationKind}
          onClose={() => setDialogOpen(false)}
        />
      )}
    </div>
  );
}
