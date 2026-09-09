import type { ReactNode } from "react";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import type { CommandContext } from "../../lib/commands/registry";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { useTheme } from "../../lib/store/useTheme";
import { useUiStore } from "../../lib/store/uiStore";
import { ChatInput } from "./ChatInput";
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
      return ipc.submitTask(sessionId, input);
    },
    onSuccess: (result) => {
      // Deltas already streamed the answer; only fall back to finalText when
      // nothing arrived live (e.g. non-streaming providers).
      if (!stream.hasLiveActivity() && result.finalText.length > 0) setFinalText(result.finalText);
      stream.clearLive();
      if (sessionId !== null) void qc.invalidateQueries({ queryKey: ["sessionEvents", sessionId] });
    },
    onError: (e) => toast.error(`${t("chat.sendFailed")}: ${describeError(e)}`),
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
      <div className="min-h-0 flex-1">
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
      <ChatInput
        disabled={false}
        pending={submitMut.isPending}
        commandContext={commandContext}
        onSubmit={async (input) => {
          await submitMut.mutateAsync(input);
        }}
      />
    </div>
  );
}
