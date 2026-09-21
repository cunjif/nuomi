import type { ReactNode } from "react";
import { useMemo, useState } from "react";
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
import { QueueList } from "../conversation/composer/QueueList";
import { MessageList } from "./MessageList";
import { STREAM_ENTRY_ID, useSessionStream, type ChatEntry } from "./useSessionStream";

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

  const commandContext = useMemo<CommandContext>(
    () => ({ sessionId, ipc, queryClient: qc, navigate, selectSession, toggleTheme: cycleTheme, toast, t }),
    [sessionId, qc, navigate, selectSession, cycleTheme, t],
  );

  const submitMut = useMutation({
    mutationFn: (input: string) => {
      if (sessionId === null) return Promise.reject(new Error("no session"));
      setFinalText(null);
      stream.addOptimistic(input);
      return ipc.submitTask(sessionId, input);
    },
    onSuccess: (result) => {
      const hadLive = stream.hasLiveActivity();
      if (!hadLive && result.finalText.length > 0) setFinalText(result.finalText);
      stream.clearLive();
      // P1-3: clear the optimistic bubble now that the turn has completed
      // and the user message is persisted. Without this, the history
      // refetch lands with the assistant reply as the last entry, which
      // makes `showOptimistic` true again and re-renders the user bubble
      // as a ghost after the assistant reply.
      stream.clearOptimistic();
      if (sessionId !== null) void qc.invalidateQueries({ queryKey: ["sessionEvents", sessionId] });
      if (!hadLive && result.finalText.length === 0) {
        toast.warn(t("chat.emptyResponse"));
      }
    },
    onError: (e) => {
      stream.clearOptimistic();
      toast.error(`${t("chat.sendFailed")}: ${describeError(e)}`);
    },
  });

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
          isEmpty={entries.length === 0}
          emptyLabel={t("chat.noMessages")}
          onRetry={stream.retry}
        >
          <MessageList entries={entries} streamingId={submitMut.isPending ? STREAM_ENTRY_ID : null} />
        </AsyncBoundary>
      </div>
      <Composer
        disabled={false}
        pending={false}
        commandContext={commandContext}
        sessionId={sessionId}
        topSlot={<QueueList sessionId={sessionId} />}
        onSubmit={async (input) => {
          if (isAgentBusy) {
            await enqueueMut.mutateAsync(input);
          } else {
            // Fire-and-forget: the optimistic bubble gives immediate
            // feedback and the composer clears right away; the turn keeps
            // running in the background (failure surfaces via toast).
            submitMut.mutate(input);
          }
        }}
      />
    </div>
  );
}
