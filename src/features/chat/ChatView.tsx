import type { ReactNode } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import type { CommandContext } from "../../lib/commands/registry";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { useTheme } from "../../lib/store/useTheme";
import { useUiStore } from "../../lib/store/uiStore";
import { Composer } from "../conversation/composer/Composer";
import { QuotePreviewBar } from "../conversation/composer/QuotePreviewBar";
import { QueueList } from "../conversation/composer/QueueList";
import { MessageList } from "./MessageList";
import { STREAM_ENTRY_ID, useSessionStream, type ChatEntry } from "./useSessionStream";
import { ChatStatusPill } from "./ChatStatusPill";
import { TurnNotice, type TurnNoticeKind } from "./TurnNotice";

interface TurnNoticeState {
  kind: TurnNoticeKind;
  summary?: string;
}

/** U9 conversation surface: bubble stream + composer + live token stream. */
export function ChatView(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const sessionId = useUiStore((s) => s.selectedSessionId);
  const navigate = useUiStore((s) => s.setView);
  const selectSession = useUiStore((s) => s.selectSession);
  const { cycleTheme } = useTheme();
  const stream = useSessionStream(sessionId);
  const [finalText, setFinalText] = useState<string | null>(null);
  const [quotedEventSeq, setQuotedEventSeq] = useState<number | null>(null);
  const [quotedExcerpt, setQuotedExcerpt] = useState<string | null>(null);
  const [rollbackTargetSeq, setRollbackTargetSeq] = useState<number | null>(null);
  // Terminal turn outcome shown inline where the pill sat (SPEC 5.8.1-5,
  // 5.8.3-1/3): stopped / failed / interrupted.
  const [turnNotice, setTurnNotice] = useState<TurnNoticeState | null>(null);
  const [turnStopped, setTurnStopped] = useState(false);
  const stoppedRef = useRef(false);
  const lastInputRef = useRef("");

  // `finalText` is the non-streaming fallback bubble for the turn that just
  // completed. It belongs to one session: ChatView stays mounted across
  // session switches, so without this reset the previous conversation's
  // answer would be appended to the newly selected transcript.
  useEffect(() => {
    setFinalText(null);
    setTurnNotice(null);
    setTurnStopped(false);
    stoppedRef.current = false;
  }, [sessionId]);

  const focusedWorkspaceId = useUiStore((s) => s.focusedWorkspaceId);
  const commandContext = useMemo<CommandContext>(
    () => ({ sessionId, focusedWorkspaceId, ipc, queryClient: qc, navigate, selectSession, toggleTheme: cycleTheme, toast, t }),
    [sessionId, focusedWorkspaceId, qc, navigate, selectSession, cycleTheme, t],
  );

  // SPEC 5.8.1-5a: stop the in-flight turn. The pill closes immediately
  // (stopped prop) and the "stopped" notice renders without waiting for
  // the backend to acknowledge the cancellation.
  const stopTurn = useCallback(() => {
    if (sessionId === null) return;
    stoppedRef.current = true;
    setTurnStopped(true);
    setTurnNotice({ kind: "stopped" });
    void ipc.stopConversation(sessionId);
  }, [sessionId]);

  const submitMut = useMutation({
    mutationFn: (input: string) => {
      if (sessionId === null) return Promise.reject(new Error("no session"));
      setFinalText(null);
      stream.addOptimistic(input);
      const qseq = quotedEventSeq;
      return ipc.submitTask(sessionId, input, qseq).then((result) => {
        setQuotedEventSeq(null);
        setQuotedExcerpt(null);
        return result;
      });
    },
    onMutate: (input) => {
      stoppedRef.current = false;
      setTurnStopped(false);
      setTurnNotice(null);
      lastInputRef.current = input;
    },
    onSuccess: (result) => {
      const hadLive = stream.hasLiveActivity();
      if (stoppedRef.current) {
        // User-stopped turn (SPEC 5.8.1-5a): no final-text insertion, no
        // empty-response toast — the "stopped" notice stays.
        stream.clearLive();
        stream.clearOptimistic();
        if (sessionId !== null) {
          void qc.invalidateQueries({ queryKey: ["sessionEvents", sessionId] });
        }
        return;
      }
      if (!hadLive && result.finalText.length > 0) setFinalText(result.finalText);
      stream.clearLive();
      // P1-3: clear the optimistic bubble now that the turn has completed
      // and the user message is persisted. Without this, the history
      // refetch lands with the assistant reply as the last entry, which
      // makes `showOptimistic` true again and re-renders the user bubble
      // as a ghost after the assistant reply.
      stream.clearOptimistic();
      if (sessionId !== null) {
        void qc.invalidateQueries({ queryKey: ["sessionEvents", sessionId] });
        // Refresh the conversations list so ChatTabBar picks up the
        // auto-derived session title after the first message.
        void qc.invalidateQueries({ queryKey: ["conversations"] });
      }
      if (!hadLive && result.finalText.length === 0) {
        toast.warn(t("chat.emptyResponse"));
      }
    },
    onError: (e) => {
      stream.clearOptimistic();
      if (!stoppedRef.current) {
        // Failed with nothing received → failure notice with summary
        // (SPEC 5.8.3-1c); failed mid-stream → keep the received text and
        // flag the interruption (SPEC 5.8.3-3c).
        const interrupted = stream.hasLiveActivity();
        setTurnNotice({
          kind: interrupted ? "interrupted" : "failed",
          summary: interrupted ? undefined : describeError(e),
        });
      }
      toast.error(`${t("chat.sendFailed")}: ${describeError(e)}`);
    },
  });
  // SPEC 5.8.3-1c/3c: retry re-sends the last input. The live buffers are
  // cleared first so a partially-received interrupted reply cannot splice
  // onto the fresh answer.
  const retryTurn = (): void => {
    if (lastInputRef.current === "") return;
    stream.clearLive();
    submitMut.mutate(lastInputRef.current);
  };

  // ADR 0015: message queue — enqueue when agent is busy.
  const queueQuery = useQuery({
    queryKey: ["messageQueue", sessionId],
    queryFn: () => (sessionId !== null ? ipc.listMessageQueue(sessionId) : Promise.resolve([])),
    enabled: sessionId !== null,
  });
  const enqueueMut = useMutation({
    mutationFn: (input: string) => {
      if (sessionId === null) return Promise.reject(new Error("no session"));
      return ipc.enqueueMessage(sessionId, input);
    },
    onSuccess: () => {
      if (sessionId !== null) void qc.invalidateQueries({ queryKey: ["messageQueue", sessionId] });
    },
    onError: (e) => toast.error(`${t("chat.sendFailed")}: ${describeError(e)}`),
  });
  const isAgentBusy = submitMut.isPending || (queueQuery.data?.length ?? 0) > 0;

  const rollbackMut = useMutation({
    mutationFn: (toSeq: number) => {
      if (sessionId === null) return Promise.reject(new Error("no session"));
      return ipc.rollbackSessionEvents(sessionId, toSeq);
    },
    onSuccess: (result) => {
      setRollbackTargetSeq(null);
      if (sessionId !== null) {
        void qc.invalidateQueries({ queryKey: ["sessionEvents", sessionId] });
      }
      toast.success(t("chat.rolledBackN", { count: result.rolledBackCount }));
    },
    onError: (e) => toast.error(`${t("chat.rollbackFailed")}: ${describeError(e)}`),
  });

  if (sessionId === null) {
    return (
      <div className="flex h-full items-center justify-center p-6 text-sm text-ink-muted">
        {t("chat.noSession")}
      </div>
    );
  }

  const entries: ChatEntry[] =
    finalText !== null
      ? [...stream.entries, { id: "__final__", kind: "message", role: "assistant", text: finalText }]
      : stream.entries;

  return (
    <div className="flex h-full flex-col">
      <div className="relative min-h-0 flex-1">
        <AsyncBoundary
          isLoading={stream.isLoading}
          error={stream.error}
          // A terminal turn notice (stopped/failed/interrupted) must stay
          // visible even when the transcript is empty — the empty state
          // must not swallow it.
          isEmpty={entries.length === 0 && turnNotice === null}
          emptyLabel={t("chat.noMessages")}
          onRetry={stream.retry}
        >
          <MessageList
            entries={entries}
            streamingId={submitMut.isPending ? STREAM_ENTRY_ID : null}
            onQuote={(seq) => {
              const entry = entries.find((e) => e.id === `m${seq}`);
              if (entry) {
                setQuotedEventSeq(seq);
                setQuotedExcerpt((entry.text ?? "").slice(0, 80));
              }
            }}
            onQuoteClick={(seq) => {
              const el = document.querySelector(`[data-entry-id="m${seq}"]`);
              el?.scrollIntoView({ behavior: "smooth", block: "center" });
            }}
            onRollback={(seq) => setRollbackTargetSeq(seq)}
          />
          <ChatStatusPill
            isPending={submitMut.isPending}
            hasStreamText={stream.hasStreamText}
            hasError={submitMut.isError}
            entries={stream.entries}
            stopped={turnStopped}
            onStop={stopTurn}
          />
          {turnNotice !== null && (
            <div className="py-1" data-testid="turn-notice-slot">
              <TurnNotice
                kind={turnNotice.kind}
                summary={turnNotice.summary}
                onRetry={retryTurn}
              />
            </div>
          )}
        </AsyncBoundary>
      </div>
      <Composer
        disabled={false}
        pending={false}
        commandContext={commandContext}
        sessionId={sessionId}
        topSlot={
          <>
            {quotedExcerpt !== null && (
              <QuotePreviewBar
                excerpt={quotedExcerpt}
                onCancel={() => {
                  setQuotedEventSeq(null);
                  setQuotedExcerpt(null);
                }}
              />
            )}
            <QueueList sessionId={sessionId} />
          </>
        }
        onSubmit={async (input) => {
          if (isAgentBusy) {
            await enqueueMut.mutateAsync(input);
          } else {
            // Fire-and-forget: the optimistic bubble gives immediate
            // feedback and the composer clears right away; the turn keeps
            // running in the background (failure surfaces via toast).
            submitMut.mutate(input);
            // The backend derives the session title from the user input
            // BEFORE running the Loop Engine (try_derive_title_early), so
            // the new title is in the DB almost immediately. But
            // submitTask only resolves after the whole turn completes,
            // which would delay the tab/list title refresh until the
            // assistant replies. Invalidate now (and again shortly to
            // cover the IPC ordering race) so the tab title refreshes
            // instantly without waiting for the RoleAgent response.
            void qc.invalidateQueries({ queryKey: ["conversations"] });
            window.setTimeout(
              () => void qc.invalidateQueries({ queryKey: ["conversations"] }),
              200,
            );
          }
        }}
      />
      {rollbackTargetSeq !== null && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
          role="dialog"
          aria-modal="true"
        >
          <div className="w-full max-w-sm rounded border border-ink-muted/40 bg-surface-raised p-4">
            <h4 className="mb-2 text-sm font-semibold text-ink">{t("chat.rollbackConfirm")}</h4>
            <p className="mb-3 text-xs text-ink-muted">
              {t("chat.rollbackHint", { count: entries.filter((e) => {
                const m = /^m(\d+)$/.exec(e.id);
                return m !== null && Number(m[1]) > rollbackTargetSeq;
              }).length })}
            </p>
            {rollbackMut.data?.containsToolCalls === true && (
              <p className="mb-3 text-xs text-amber-500">
                {t("chat.toolSideEffectsRetained")}
              </p>
            )}
            <div className="flex justify-end gap-2">
              <button
                type="button"
                onClick={() => setRollbackTargetSeq(null)}
                className="rounded border border-ink-muted px-3 py-1 text-sm text-ink-muted hover:bg-surface-overlay"
              >
                {t("common.cancel")}
              </button>
              <button
                type="button"
                disabled={rollbackMut.isPending}
                onClick={() => rollbackMut.mutate(rollbackTargetSeq)}
                className="pixel-fill-accent px-3 py-1 text-sm text-surface disabled:opacity-50"
              >
                {rollbackMut.isPending ? t("common.processing") : t("chat.rollbackToHere")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
