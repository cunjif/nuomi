import type { ReactNode } from "react";
import { useEffect, useMemo } from "react";
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
import { SpeakerBubble } from "./SpeakerBubble";
import { RoundIndicator } from "./RoundIndicator";
import { WhiteboardDock } from "./WhiteboardDock";
import { GroupControls } from "./GroupControls";

/** Stable color from role id hash. */
function roleColor(roleId: string): string {
  const colors = ["#e76f51", "#2a9d8f", "#264653", "#e9c46a", "#457b9d", "#a8dadc", "#f4a261"];
  let hash = 0;
  for (const ch of roleId) hash = (hash * 31 + ch.charCodeAt(0)) | 0;
  return colors[Math.abs(hash) % colors.length]!;
}

/**
 * Group conversation view: speaker timeline + whiteboard dock + round indicator.
 * Uses the enhanced Composer for message input (reuses @ routing, attachments,
 * context injection, etc.).
 */
export function GroupConversationView(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const sessionId = useUiStore((s) => s.selectedSessionId);
  const navigate = useUiStore((s) => s.setView);
  const selectSession = useUiStore((s) => s.selectSession);
  const { cycleTheme } = useTheme();

  const commandContext = useMemo<CommandContext>(
    () => ({ sessionId, ipc, queryClient: qc, navigate, selectSession, toggleTheme: cycleTheme, toast, t }),
    [sessionId, qc, navigate, selectSession, cycleTheme, t],
  );

  const eventsQuery = useQuery({
    queryKey: ["sessionEvents", sessionId, "group"],
    queryFn: () => (sessionId ? ipc.listEvents(sessionId, 0) : Promise.resolve([])),
    enabled: sessionId !== null,
    refetchInterval: 3_000,
  });

  const submitMut = useMutation({
    mutationFn: (input: string) => {
      if (sessionId === null) return Promise.reject(new Error("no session"));
      return ipc.submitMessage(sessionId, input, [], null, null);
    },
    onSuccess: () => {
      if (sessionId !== null) void qc.invalidateQueries({ queryKey: ["sessionEvents", sessionId, "group"] });
    },
    onError: (e) => toast.error(`${t("chat.sendFailed")}: ${describeError(e)}`),
  });

  if (sessionId === null) {
    return <div className="flex h-full items-center justify-center text-sm text-ink-muted">{t("chat.noSession")}</div>;
  }

  const events = eventsQuery.data ?? [];
  const messages = events.filter((e) => e.kind === "message");

  const { scrollRef, scrollToBottomIfStuck } = useStickToBottom();
  const lastSeq = messages[messages.length - 1]?.seq ?? 0;
  useEffect(() => {
    scrollToBottomIfStuck();
  }, [messages.length, lastSeq, scrollToBottomIfStuck]);

  return (
    <div className="flex h-full flex-col">
      <RoundIndicator current={1} max={6} />
      <div className="flex min-h-0 flex-1">
        <div ref={scrollRef} className="relative min-h-0 flex-1 overflow-y-auto p-2">
          {messages.map((msg) => {
            const role = (msg.payload as { role?: string }).role ?? "assistant";
            const content = (msg.payload as { content?: string }).content ?? "";
            return (
              <SpeakerBubble
                key={msg.seq}
                roleName={role}
                roleColor={roleColor(role)}
                content={content}
              />
            );
          })}
          {messages.length === 0 && (
            <p className="py-4 text-center text-sm text-ink-muted">{t("chat.noMessages")}</p>
          )}
        </div>
        <WhiteboardDock sessionId={sessionId} />
      </div>
      <GroupControls onStop={() => void ipc.stopConversation(sessionId)} running={false} />
      <Composer
        disabled={false}
        pending={submitMut.isPending}
        commandContext={commandContext}
        sessionId={sessionId}
        onSubmit={async (input) => {
          await submitMut.mutateAsync(input);
        }}
      />
    </div>
  );
}
