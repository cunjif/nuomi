import { Fragment, useState, useRef, useMemo, useEffect, type ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { ipc } from "../../lib/ipc/client";
import { useUiStore } from "../../lib/store/uiStore";
import { workspaceBadgeColor } from "../conversation/workspaceBadgeColor";
import { ChatTab } from "./ChatTab";
import { ConnectionStatusBadge } from "./ConnectionStatusBadge";
import { NewConversationDialog } from "../conversation/new/NewConversationDialog";
import type { ConversationKind } from "../../lib/conversation/kinds";

/** Virtualization threshold — below this, render all tabs. */
const TAB_VIRTUALIZE_THRESHOLD = 30;
/** Assumed average tab width (px) for virtualization windowing. */
const ASSUMED_TAB_WIDTH = 120;

/**
 * Multi-chat tab bar rendered in the strip row above the tab panel.
 * Three-segment layout: horizontal-scroll tab list (bottom-aligned 40px
 * chips, hairline-divided) + fixed-right connection badge. Neither the strip
 * row nor the panel draws a line between them; the "+" button matches the
 * chip height.
 *
 * ADR-0021 task 6.9 — the strip is workspace-scoped: metadata comes from
 * `listConversations(null, activeWorkspaceId)`, so only tabs whose session
 * belongs to the open workspace render. `openSessionIds` itself stays global
 * (switching workspaces hides, never closes, tabs — spec 5.5.1-2). The empty
 * state (no workspace open) falls back to the cross-workspace listing.
 */
export function ChatTabBar(): ReactNode {
  const { t } = useTranslation();
  const openSessionIds = useUiStore((s) => s.openSessionIds);
  const selectedSessionId = useUiStore((s) => s.selectedSessionId);
  const activeWorkspaceId = useUiStore((s) => s.activeWorkspaceId);
  const activateChatTab = useUiStore((s) => s.activateChatTab);
  const closeChatTab = useUiStore((s) => s.closeChatTab);
  const [dialogOpen, setDialogOpen] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  const { data: conversations } = useQuery({
    queryKey: ["conversations", null, activeWorkspaceId],
    queryFn: () => ipc.listConversations(null, activeWorkspaceId),
    staleTime: 10_000,
  });

  const workspacesQuery = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => ipc.listWorkspaces(),
    staleTime: 30_000,
  });
  const workspaceLabelMap = useMemo(
    () =>
      new Map(
        (workspacesQuery.data ?? []).map((ws) => [ws.id, ws.labelFull ?? ws.rootPath]),
      ),
    [workspacesQuery.data],
  );

  const convMap = useMemo(() => {
    const m = new Map((conversations ?? []).map((c) => [c.id, c]));
    return m;
  }, [conversations]);

  // Workspace scoping: once the workspace-scoped query resolves, only open
  // tabs whose session belongs to the workspace render. While the query is
  // pending the strip stays empty (tabs reappear when data lands — the strip
  // switches as a whole, spec 5.5.1-2). The empty state keeps the legacy
  // cross-workspace view with fallback metadata.
  const scopedIds = useMemo(() => {
    if (activeWorkspaceId === null) return openSessionIds;
    if (conversations === undefined) return [];
    return openSessionIds.filter((id) => convMap.has(id));
  }, [activeWorkspaceId, openSessionIds, conversations, convMap]);

  // Workspace switch re-homes the selection: a stale selection (pointing at
  // a tab of the previous workspace, or none) activates the first surviving
  // tab. `activateChatTab` is used instead of `selectSession` because the
  // latter has an activeArea side effect — switching workspaces must not
  // yank the user out of the editor. An empty strip keeps the stale
  // selection so switching back restores it.
  useEffect(() => {
    const first = scopedIds[0];
    if (first !== undefined && !scopedIds.includes(selectedSessionId ?? "")) {
      activateChatTab(first);
    }
  }, [scopedIds, selectedSessionId, activateChatTab]);

  // Virtualization: when tab count exceeds threshold, window based on scrollLeft.
  const [scrollLeft, setScrollLeft] = useState(0);
  const viewportWidth = scrollRef.current?.clientWidth ?? 0;
  const virtualize = scopedIds.length > TAB_VIRTUALIZE_THRESHOLD;
  const startIdx = virtualize
    ? Math.max(0, Math.floor(scrollLeft / ASSUMED_TAB_WIDTH) - 2)
    : 0;
  const endIdx = virtualize
    ? Math.min(
        scopedIds.length,
        startIdx + Math.ceil((viewportWidth + 4 * ASSUMED_TAB_WIDTH) / ASSUMED_TAB_WIDTH),
      )
    : scopedIds.length;
  const visibleIds = scopedIds.slice(startIdx, endIdx);

  return (
    <div className="flex w-full items-center gap-1">
      {/* Tab scroll region. items-end keeps the 40px chips flush against the
          panel's top border while the "+" button shares the same height. */}
      <div
        ref={scrollRef}
        onScroll={(e) => setScrollLeft((e.target as HTMLDivElement).scrollLeft)}
        className="flex min-w-0 flex-1 items-end gap-1.5 overflow-x-auto"
        role="tablist"
        aria-label={t("nav.chatTabsLabel")}
      >
        {visibleIds.map((id, i) => {
          const conv = convMap.get(id);
          return (
            <Fragment key={id}>
              {/* Hairline between neighbouring tabs — never before the first
                  one, and never around the "+" button. `bg-divider` carries the
                  per-theme tint (see --nuomi-divider); `self-center` centres the
                  16px rule in the 40px chip's row despite items-end. */}
              {i > 0 && (
                <span
                  aria-hidden="true"
                  data-testid="chat-tab-divider"
                  className="h-4 w-px shrink-0 self-center bg-divider"
                />
              )}
              <ChatTab
                sessionId={id}
                kind={conv?.kind ?? "chat"}
                title={conv?.title ?? id}
                workspaceColor={conv !== undefined ? workspaceBadgeColor(conv.workspaceId) : null}
                workspaceLabel={
                  conv !== undefined ? (workspaceLabelMap.get(conv.workspaceId) ?? null) : null
                }
                active={selectedSessionId === id}
                onActivate={activateChatTab}
                onClose={closeChatTab}
              />
            </Fragment>
          );
        })}
        {/* "+" new conversation button — matches the 40px chip height so the
            strip row keeps a single baseline. */}
        <button
          type="button"
          onClick={() => setDialogOpen(true)}
          className="flex h-10 w-9 shrink-0 items-center justify-center rounded-md border border-transparent text-base leading-none text-ink-muted transition-colors hover:bg-surface-overlay hover:text-ink-accent focus-visible:ring-2 focus-visible:ring-ink-accent"
          aria-label={t("conversation.new")}
          title={t("conversation.new")}
        >
          +
        </button>
      </div>
      {/* Connection status fixed right (does not scroll). */}
      <div className="flex shrink-0 items-center pb-0.5">
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
