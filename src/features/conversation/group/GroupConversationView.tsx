import type { ReactNode } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { CommandContext } from "../../../lib/commands/registry";
import { describeError } from "../../../i18n";
import { ipc } from "../../../lib/ipc/client";
import { toast } from "../../../lib/store/toastStore";
import { useTheme } from "../../../lib/store/useTheme";
import { useUiStore } from "../../../lib/store/uiStore";
import { useStickToBottom } from "../../../components/ui/useStickToBottom";
import { Composer } from "../composer/Composer";
import { QuotePreviewBar } from "../composer/QuotePreviewBar";
import { SpeakerBubble } from "./SpeakerBubble";
import { RoundIndicator } from "./RoundIndicator";
import { WhiteboardDock } from "./WhiteboardDock";
import { GroupControls } from "./GroupControls";
import { StatusPill, type StatusPillEntry } from "../../chat/StatusPill";
import { TurnNotice, type TurnNoticeKind } from "../../chat/TurnNotice";
import { seqFromId, useSessionStream } from "../../chat/useSessionStream";
import { hashPaletteColor } from "../avatarPalette";

interface TurnNoticeState {
  kind: TurnNoticeKind;
  summary?: string;
}

const PILL_FADE_MS = 220;

/**
 * Group conversation view: speaker timeline + whiteboard dock + round
 * indicator. Consumes the ADR-0002 session event stream (live deltas,
 * selector decisions, durable messages) via useSessionStream — no polling.
 * While a run is in flight a scrolling status pill tracks the nominated
 * speaker (SPEC 5.8.1-4a); the first delta of each utterance fades the pill
 * out and swaps in the speaking agent's streaming bubble (5.8.1-4b), and
 * between utterances the pill returns as the next-agent waiting state
 * (5.8.1-4c).
 */
export function GroupConversationView(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const sessionId = useUiStore((s) => s.selectedSessionId);
  // Bound work-directory id (chat_rich_bind 6.4) — same query key as
  // ChatView's convQuery → shared cache, no extra fetch.
  const convQuery = useQuery({
    queryKey: ["conversation", sessionId],
    queryFn: () =>
      sessionId !== null
        ? ipc.getConversation(sessionId)
        : Promise.reject(new Error("no session")),
    enabled: sessionId !== null,
    staleTime: 10_000,
  });
  const navigate = useUiStore((s) => s.setView);
  const selectSession = useUiStore((s) => s.selectSession);
  const { cycleTheme } = useTheme();
  const [quotedEventSeq, setQuotedEventSeq] = useState<number | null>(null);
  const [quotedExcerpt, setQuotedExcerpt] = useState<string | null>(null);
  // WeChat-style bubble toolbar: at most one bubble's toolbar is open at a
  // time; the id tracks which message owns it.
  const [toolbarEntryId, setToolbarEntryId] = useState<string | null>(null);

  const [pillEntries, setPillEntries] = useState<StatusPillEntry[]>([]);
  const [pillStartedAt, setPillStartedAt] = useState(0);
  const [pillClosing, setPillClosing] = useState(false);
  const pillStartedAtRef = useRef(0);
  const submitPendingRef = useRef(false);
  // Terminal turn outcome shown where the pill sat (SPEC 5.8.1-5, 5.8.3-1/3).
  const [turnNotice, setTurnNotice] = useState<TurnNoticeState | null>(null);
  const stoppedRef = useRef(false);
  const lastInputRef = useRef("");
  /** Any live output seen THIS turn (delta or durable utterance) — the
   *  failed-vs-interrupted discriminator (SPEC 5.8.3-1 vs 5.8.3-3). */
  const sawActivityRef = useRef(false);

  const activeWorkspaceId = useUiStore((s) => s.activeWorkspaceId);
  const commandContext = useMemo<CommandContext>(
    () => ({ sessionId, activeWorkspaceId, ipc, queryClient: qc, navigate, selectSession, toggleTheme: cycleTheme, toast, t }),
    [sessionId, activeWorkspaceId, qc, navigate, selectSession, cycleTheme, t],
  );

  const { entries, groupStream, activeSpeaker, lastMessageSeq, addOptimistic, clearOptimistic, clearLive } =
    useSessionStream(sessionId);

  /** Appends one status line, re-seeding the pill when it was cleared mid-run. */
  const pushPill = useCallback((text: string) => {
    setPillEntries((prev) =>
      prev.length === 0 ? [{ text, at: Date.now() }] : [...prev, { text, at: Date.now() }],
    );
  }, []);

  // SPEC 5.8.1-5a: stop the in-flight group turn. The pill closes
  // immediately and the "stopped" notice renders without waiting for the
  // backend race to settle; no further streaming content is inserted.
  const stopTurn = useCallback(() => {
    if (sessionId === null) return;
    stoppedRef.current = true;
    setPillEntries([]);
    setPillClosing(false);
    setTurnNotice({ kind: "stopped" });
    void ipc.stopConversation(sessionId);
  }, [sessionId]);

  const submitMut = useMutation({
    mutationFn: (input: string) => {
      if (sessionId === null) return Promise.reject(new Error("no session"));
      return ipc.submitMessage(sessionId, input, [], null, null, quotedEventSeq).then((result) => {
        setQuotedEventSeq(null);
        setQuotedExcerpt(null);
        return result;
      });
    },
    onMutate: (input) => {
      stoppedRef.current = false;
      sawActivityRef.current = false;
      setTurnNotice(null);
      lastInputRef.current = input;
      addOptimistic(input);
      const now = Date.now();
      pillStartedAtRef.current = now;
      setPillStartedAt(now);
      setPillClosing(false);
      setPillEntries([{ text: t("chat.status.group.waiting"), at: now }]);
    },
    onSuccess: () => {
      if (sessionId !== null) {
        void qc.invalidateQueries({ queryKey: ["sessionEvents", sessionId] });
        // Refresh the conversations list so ChatTabBar picks up the
        // auto-derived session title after the first message.
        void qc.invalidateQueries({ queryKey: ["conversations"] });
      }
    },
    onError: (e) => {
      if (!stoppedRef.current) {
        // Nothing produced this turn → failure notice with the error
        // summary (SPEC 5.8.3-1c); output already received → keep it and
        // flag the interruption (SPEC 5.8.3-3c).
        const interrupted = sawActivityRef.current;
        setTurnNotice({
          kind: interrupted ? "interrupted" : "failed",
          summary: interrupted ? undefined : describeError(e),
        });
      }
      toast.error(`${t("chat.sendFailed")}: ${describeError(e)}`);
    },
    onSettled: () => {
      clearOptimistic();
      pillStartedAtRef.current = 0;
      setPillEntries([]);
      setPillStartedAt(0);
    },
  });
  submitPendingRef.current = submitMut.isPending;

  // Track live output for the failed-vs-interrupted discriminator. The
  // selector nomination alone is NOT output — a turn failing before any
  // utterance is a plain failure.
  useEffect(() => {
    if (!submitPendingRef.current) return;
    if (groupStream !== null || lastMessageSeq > 0) sawActivityRef.current = true;
  }, [groupStream, lastMessageSeq]);

  // SPEC 5.8.3-1c/3c: retry re-sends the last input; the live group buffer
  // is cleared so a re-nominated speaker's fresh utterance cannot splice
  // onto the interrupted one.
  const retryTurn = (): void => {
    if (lastInputRef.current === "") return;
    clearLive();
    submitMut.mutate(lastInputRef.current);
  };

  // Selector nomination → new pill line naming the upcoming speaker.
  const speakerName = activeSpeaker?.roleName;
  useEffect(() => {
    if (speakerName === undefined || !submitPendingRef.current) return;
    pushPill(t("chat.status.group.speaking", { name: speakerName }));
  }, [speakerName, t, pushPill]);

  // First delta → pill fades out and the streaming bubble takes over.
  // (When an entire short utterance lands in one rAF batch the non-null
  // groupStream state is never committed, so the fade path is skipped —
  // the durable-message effect below still re-seeds the waiting pill.)
  useEffect(() => {
    if (groupStream !== null) {
      setPillClosing(true);
      const timer = setTimeout(() => setPillEntries([]), PILL_FADE_MS);
      return () => clearTimeout(timer);
    }
    setPillClosing(false);
    return undefined;
  }, [groupStream]);

  // A durable utterance landed while the run is in flight → the pill
  // returns as the next-agent waiting state (SPEC 5.8.1-4c).
  useEffect(() => {
    if (lastMessageSeq <= 0 || !submitPendingRef.current) return;
    pushPill(t("chat.status.group.next"));
  }, [lastMessageSeq, t, pushPill]);

  // Session switch: drop all live pill state (the stream hook resets too).
  useEffect(() => {
    setPillEntries([]);
    setPillStartedAt(0);
    setPillClosing(false);
    setTurnNotice(null);
    setQuotedEventSeq(null);
    setQuotedExcerpt(null);
    setToolbarEntryId(null);
    pillStartedAtRef.current = 0;
    stoppedRef.current = false;
    sawActivityRef.current = false;
  }, [sessionId]);

  // Clicking anywhere outside a message row closes the open bubble toolbar.
  useEffect(() => {
    if (toolbarEntryId === null) return;
    const onDocMouseDown = (e: MouseEvent) => {
      if ((e.target as HTMLElement).closest("[data-entry-id]") === null) {
        setToolbarEntryId(null);
      }
    };
    document.addEventListener("mousedown", onDocMouseDown);
    return () => document.removeEventListener("mousedown", onDocMouseDown);
  }, [toolbarEntryId]);

  const { scrollRef, scrollToBottomIfStuck } = useStickToBottom();
  const messages = entries.filter((e) => e.kind === "message");

  /** Quote toolbar action: stage the referenced message for the next submit. */
  const handleQuote = (seq: number): void => {
    const msg = messages.find((m) => m.id === `m${seq}` || m.id === `live-m${seq}`);
    if (msg === undefined) return;
    setQuotedEventSeq(seq);
    setQuotedExcerpt((msg.text ?? "").slice(0, 80));
  };

  /** Clicking a quote block scrolls the original message into view. */
  const scrollToQuoted = (seq: number): void => {
    const el = document.querySelector(
      `[data-entry-id="m${seq}"], [data-entry-id="live-m${seq}"]`,
    );
    el?.scrollIntoView({ behavior: "smooth", block: "center" });
  };

  const lastEntryId = messages[messages.length - 1]?.id ?? "";
  const streamLen = groupStream?.text.length ?? 0;
  useEffect(() => {
    scrollToBottomIfStuck();
  }, [messages.length, lastEntryId, streamLen, pillEntries.length, scrollToBottomIfStuck]);

  if (sessionId === null) {
    return <div className="flex h-full items-center justify-center text-sm text-ink-muted">{t("chat.noSession")}</div>;
  }

  return (
    <div className="flex h-full flex-col">
      <RoundIndicator current={1} max={6} />
      <div className="flex min-h-0 flex-1">
        <div ref={scrollRef} className="relative min-h-0 flex-1 overflow-y-auto px-2 pt-1 pb-2">
          {messages.map((msg, index) => {
            const roleName = msg.roleName ?? "assistant";
            // WeChat-style grouping: a gap opens when the previous message
            // spoke as the other side (user ↔ agent). Agent-to-agent
            // handoffs stay compact.
            const prevMsg = messages[index - 1];
            const roleGap =
              prevMsg !== undefined &&
              (prevMsg.role === "user") !== (msg.role === "user");
            return (
              <div key={msg.id} data-entry-id={msg.id}>
                <SpeakerBubble
                  roleName={roleName}
                  roleColor={hashPaletteColor(roleName)}
                  content={msg.text ?? ""}
                  quotedExcerpt={msg.quotedExcerpt}
                  onQuoteClick={msg.quotedEventSeq !== undefined ? () => scrollToQuoted(msg.quotedEventSeq!) : undefined}
                  seq={seqFromId(msg.id)}
                  onQuote={handleQuote}
                  toolbarOpen={toolbarEntryId === msg.id}
                  onToolbarToggle={() => setToolbarEntryId((prev) => (prev === msg.id ? null : msg.id))}
                  onToolbarClose={() => setToolbarEntryId(null)}
                  roleGap={roleGap}
                />
              </div>
            );
          })}
          {groupStream !== null && (
            <SpeakerBubble
              roleName={groupStream.roleName}
              roleColor={hashPaletteColor(groupStream.roleName)}
              content={groupStream.text}
            />
          )}
          {pillEntries.length > 0 && (
            <div className="py-1" data-testid="group-pill-slot">
              <StatusPill
                entries={pillEntries}
                startedAt={pillStartedAt}
                closing={pillClosing}
                onStop={stopTurn}
              />
            </div>
          )}
          {turnNotice !== null && (
            <div className="py-1" data-testid="turn-notice-slot">
              <TurnNotice
                kind={turnNotice.kind}
                summary={turnNotice.summary}
                onRetry={retryTurn}
              />
            </div>
          )}
          {messages.length === 0 && groupStream === null && pillEntries.length === 0 && turnNotice === null && (
            <p className="py-4 text-center text-sm text-ink-muted">{t("chat.noMessages")}</p>
          )}
        </div>
        <WhiteboardDock sessionId={sessionId} />
      </div>
      <GroupControls onStop={stopTurn} running={submitMut.isPending} />
      <Composer
        disabled={false}
        pending={submitMut.isPending}
        commandContext={commandContext}
        sessionId={sessionId}
        workDirectoryId={convQuery.data?.workDirectoryId ?? null}
        topSlot={
          quotedExcerpt !== null ? (
            <QuotePreviewBar
              excerpt={quotedExcerpt}
              onCancel={() => {
                setQuotedEventSeq(null);
                setQuotedExcerpt(null);
              }}
            />
          ) : undefined
        }
        onSubmit={async (input) => {
          await submitMut.mutateAsync(input);
        }}
      />
    </div>
  );
}
