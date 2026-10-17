import { useState, useCallback, useEffect } from "react";
import type { ReactNode } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { Icon } from "../../components/ui/Icon/Icon";
import type { StewardReplyDto } from "../../lib/ipc/bindings.gen";
import { StewardSettingsPanel } from "./StewardSettingsPanel";
import { StewardComposer } from "./StewardComposer";
import { useStewardStream } from "./useStewardStream";
import type { StewardLiveTurn } from "./useStewardStream";

interface ChatMessage {
  role: "user" | "steward";
  text: string;
  reply?: StewardReplyDto;
  /** Degrade (steward.llm_failed) or IPC error — offers one-click resend. */
  failed?: boolean;
  /** Original user text for the resend affordance. */
  resendText?: string;
}

export function StewardChat(): ReactNode {
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [ensureError, setEnsureError] = useState(false);
  const [panelOpen, setPanelOpen] = useState(false);
  const qc = useQueryClient();
  const { live, beginTurn, endTurn } = useStewardStream(currentSessionId);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const session = await ipc.stewardEnsureSingletonSession();
        if (cancelled) return;
        setCurrentSessionId(session.id);
        setEnsureError(false);
        const history = await ipc.stewardListSessionMessages(session.id, 200);
        if (cancelled) return;
        setMessages(
          history.map((msg) => ({
            role: msg.role,
            text: msg.text,
            reply: msg.reply ?? undefined,
          })),
        );
      } catch {
        if (!cancelled) setEnsureError(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const sendMut = useMutation({
    mutationFn: ({
      sessionId,
      text,
      attachmentIds,
    }: {
      sessionId: string;
      text: string;
      attachmentIds: string[];
    }) =>
      ipc.stewardSendMessage(
        sessionId,
        text,
        attachmentIds.length > 0 ? attachmentIds : undefined,
      ),
    onSuccess: (reply, vars) => {
      // The IPC return is the authoritative conclusion (design §2.2.2-A1).
      // Yield one frame before committing: the bus events published before
      // the return (`steward.llm_failed` et al.) may still sit in the rAF
      // batch pipeline — flush them first so the commit sees the degrade
      // flag.
      requestAnimationFrame(() => {
        const turn = endTurn();
        setMessages((prev) => [
          ...prev,
          { role: "user", text: vars.text },
          {
            role: "steward",
            text: replyText(reply),
            reply,
            failed: turn?.failed ?? false,
            resendText: (turn?.failed ?? false) ? vars.text : undefined,
          },
        ]);
        // Refresh the context usage indicator after the turn.
        void qc.invalidateQueries({ queryKey: ["contextUsage"] });
      });
    },
    onError: (e, vars) => {
      requestAnimationFrame(() => {
        endTurn();
        setMessages((prev) => [
          ...prev,
          { role: "user", text: vars.text },
          {
            role: "steward",
            text: `发送失败: ${e}`,
            failed: true,
            resendText: vars.text,
          },
        ]);
        // Draft and attachments live in the composer and are kept on
        // failure (spec 5.1.3 / 5.3.3-1c) — the user can remove the named
        // attachment and retry.
      });
    },
  });

  const sendText = useCallback(
    async (text: string, attachmentIds: string[]): Promise<boolean> => {
      if (!text.trim() || sendMut.isPending) return false;
      let sessionId = currentSessionId;
      if (!sessionId) {
        try {
          const session = await ipc.stewardEnsureSingletonSession();
          sessionId = session.id;
          setCurrentSessionId(session.id);
          setEnsureError(false);
        } catch {
          setEnsureError(true);
          return false;
        }
      }
      beginTurn(text);
      try {
        await sendMut.mutateAsync({ sessionId, text, attachmentIds });
      } catch {
        // onError already rendered the failure bubble; the composer keeps
        // the draft and attachments for a retry.
        return false;
      }
      return true;
    },
    [currentSessionId, sendMut, beginTurn],
  );

  const handleResend = useCallback(
    (text: string) => {
      void sendText(text, []);
    },
    [sendText],
  );

  return (
    <div className="flex h-full">
      <div className="flex min-h-0 flex-1 flex-col">
        <div className="flex shrink-0 items-center justify-end border-b border-ink-muted/20 px-3 py-1">
          <button
            type="button"
            aria-label="管家设置"
            className="rounded p-1 text-ink-muted hover:bg-surface hover:text-ink"
            onClick={() => setPanelOpen(true)}
          >
            <Icon name="more" size={16} />
          </button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-4">
          {currentSessionId === null ? (
            <div className="flex h-full items-center justify-center text-sm text-ink-muted">
              {ensureError
                ? "会话历史暂时不可用，可继续发送新消息"
                : "管家正在就绪..."}
            </div>
          ) : messages.length === 0 && live === null ? (
            <div className="flex h-full items-center justify-center text-sm text-ink-muted">
              向管家发送消息，例如"诊断应用状态"、"触发进化"等
            </div>
          ) : (
            <div className="space-y-3">
              {messages.map((msg, i) => (
                <MessageBubble
                  key={i}
                  msg={msg}
                  sessionId={currentSessionId}
                  onResend={handleResend}
                  busy={sendMut.isPending}
                />
              ))}
              {live !== null && (
                <>
                  <MessageBubble msg={{ role: "user", text: live.userText }} />
                  <LiveStewardBubble live={live} pending={sendMut.isPending} />
                </>
              )}
            </div>
          )}
        </div>
        <StewardComposer
          sessionId={currentSessionId}
          disabled={sendMut.isPending}
          onSend={sendText}
        />
      </div>
      {currentSessionId && (
        <StewardSettingsPanel
          sessionId={currentSessionId}
          open={panelOpen}
          onClose={() => setPanelOpen(false)}
        />
      )}
    </div>
  );
}

function replyText(reply: StewardReplyDto): string {
  switch (reply.type) {
    case "text":
      return reply.content;
    case "config_proposal":
      return `配置变更建议（${reply.proposal_id}）：\n${reply.diff}`;
    case "evolution_accepted":
      return `进化周期已受理，周期 ID: ${reply.cycle_id}`;
    case "clarify":
      return `请明确您的意图：${reply.candidates.map((c) => c.kind).join(", ")}`;
    case "refused":
      return `拒绝: ${reply.reason}`;
  }
}

function MessageBubble({
  msg,
  sessionId,
  onResend,
  busy,
}: {
  msg: ChatMessage;
  sessionId?: string;
  onResend?: (text: string) => void;
  busy?: boolean;
}): ReactNode {
  if (msg.role === "user") {
    return (
      <div className="flex justify-end">
        <div className="max-w-[80%] rounded bg-surface-overlay px-3 py-2 text-sm text-ink">
          {msg.text}
        </div>
      </div>
    );
  }
  const reply = msg.reply;
  if (reply && reply.type === "config_proposal" && sessionId !== undefined) {
    return <ConfigProposalBubble reply={reply} sessionId={sessionId} />;
  }
  return (
    <div className="flex justify-start">
      <div className="max-w-[80%] whitespace-pre-wrap rounded bg-surface-raised px-3 py-2 text-sm text-ink">
        {msg.text}
        {msg.failed === true && msg.resendText !== undefined && onResend !== undefined && (
          <div className="mt-2">
            <button
              type="button"
              className="rounded border border-ink-muted/40 px-2 py-0.5 text-xs text-ink-muted hover:bg-surface disabled:opacity-50"
              onClick={() => onResend(msg.resendText ?? "")}
              disabled={busy}
            >
              重发
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

/** Live (in-flight) steward bubble: streaming → tool call → authoritative takeover. */
function LiveStewardBubble({ live, pending }: { live: StewardLiveTurn; pending: boolean }): ReactNode {
  const text = live.authoritativeText ?? live.streamText;
  return (
    <div className="flex justify-start">
      <div className="max-w-[80%] rounded bg-surface-raised px-3 py-2 text-sm text-ink">
        {text.length > 0 ? (
          <>
            <span className="whitespace-pre-wrap">{text}</span>
            {live.authoritativeText === null && <span className="animate-pulse">▍</span>}
          </>
        ) : live.activeTool !== null ? (
          <span className="text-ink-muted">正在调用工具 {live.activeTool}...</span>
        ) : pending ? (
          <span className="text-ink-muted">思考中...</span>
        ) : null}
      </div>
    </div>
  );
}

function ConfigProposalBubble({
  reply,
  sessionId,
}: {
  reply: Extract<StewardReplyDto, { type: "config_proposal" }>;
  sessionId: string;
}): ReactNode {
  const qc = useQueryClient();
  const [resolution, setResolution] = useState<"pending" | "confirmed" | "cancelled">("pending");
  const confirmMut = useMutation({
    mutationFn: () => ipc.stewardConfirmConfigChange(reply.proposal_id),
    onSuccess: () => {
      setResolution("confirmed");
      toast.success("配置变更已确认");
      void qc.invalidateQueries({ queryKey: ["steward"] });
    },
    onError: (e) => toast.error(`确认失败: ${e}`),
  });
  const cancelMut = useMutation({
    mutationFn: () => ipc.stewardCancelConfigChange(sessionId, reply.proposal_id),
    onSuccess: () => {
      setResolution("cancelled");
      toast.success("已取消配置变更");
      void qc.invalidateQueries({ queryKey: ["steward"] });
    },
    onError: (e) => toast.error(`取消失败: ${e}`),
  });
  const busy = confirmMut.isPending || cancelMut.isPending;
  return (
    <div className="flex justify-start">
      <div className="max-w-[80%] rounded bg-surface-raised p-3">
        <p className="mb-2 text-sm font-medium text-ink">配置变更建议</p>
        <pre className="mb-3 max-h-60 overflow-auto rounded bg-surface px-2 py-1 text-xs text-ink-muted">
          {reply.diff}
        </pre>
        {resolution === "pending" ? (
          <div className="flex gap-2">
            <button
              className="rounded bg-ink-accent px-3 py-1 text-xs text-surface disabled:opacity-50"
              onClick={() => confirmMut.mutate()}
              disabled={busy}
            >
              {confirmMut.isPending ? "确认中..." : "确认变更"}
            </button>
            <button
              className="rounded border border-ink-muted/40 px-3 py-1 text-xs text-ink-muted hover:bg-surface disabled:opacity-50"
              onClick={() => cancelMut.mutate()}
              disabled={busy}
            >
              {cancelMut.isPending ? "取消中..." : "取消"}
            </button>
          </div>
        ) : (
          <p className="text-xs text-ink-muted">
            {resolution === "confirmed" ? "已确认生效" : "已取消，配置保持不变"}
          </p>
        )}
      </div>
    </div>
  );
}
