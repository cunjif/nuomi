/**
 * Steward help dialog: a Dialog containing a mini steward chat panel.
 * When opened, it auto-sends a context message to the steward agent
 * and displays the reply. The user can continue the conversation.
 */
import { useState, useEffect, useCallback, useRef, type ReactNode } from "react";
import { useMutation } from "@tanstack/react-query";
import { Dialog } from "../../components/ui/Dialog";
import { Icon } from "../../components/ui/Icon/Icon";
import { ipc } from "../../lib/ipc/client";
import type { StewardReplyDto } from "../../lib/ipc/bindings.gen";

interface DialogMessage {
  role: "user" | "steward";
  text: string;
  reply?: StewardReplyDto;
  failed?: boolean;
}

export interface StewardDialogProps {
  open: boolean;
  onClose: () => void;
  /** The auto-sent message when the dialog opens. */
  autoMessage: string;
}

export function StewardDialog({ open, onClose, autoMessage }: StewardDialogProps): ReactNode {
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [messages, setMessages] = useState<DialogMessage[]>([]);
  const [input, setInput] = useState("");
  const [loading, setLoading] = useState(false);
  const [initError, setInitError] = useState(false);
  const autoSentRef = useRef(false);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!open) {
      setMessages([]);
      setInput("");
      setLoading(false);
      setInitError(false);
      autoSentRef.current = false;
      return;
    }
    let cancelled = false;
    void (async () => {
      try {
        const session = await ipc.stewardEnsureSingletonSession();
        if (cancelled) return;
        setSessionId(session.id);
        setInitError(false);
      } catch {
        if (!cancelled) setInitError(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [open]);

  useEffect(() => {
    if (open && sessionId !== null && !autoSentRef.current) {
      autoSentRef.current = true;
      void sendAuto(autoMessage);
    }
  }, [open, sessionId, autoMessage]);

  useEffect(() => {
    if (scrollRef.current !== null) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages]);

  const sendMut = useMutation({
    mutationFn: ({
      sid,
      text,
    }: {
      sid: string;
      text: string;
    }) => ipc.stewardSendMessage(sid, text),
    onSuccess: (reply, vars) => {
      setMessages((prev) => [
        ...prev,
        { role: "user", text: vars.text },
        { role: "steward", text: replyText(reply), reply },
      ]);
      setLoading(false);
    },
    onError: (e, vars) => {
      setMessages((prev) => [
        ...prev,
        { role: "user", text: vars.text },
        { role: "steward", text: `发送失败: ${e}`, failed: true },
      ]);
      setLoading(false);
    },
  });

  const sendAuto = useCallback(
    async (text: string) => {
      if (!text.trim() || sessionId === null) return;
      setLoading(true);
      try {
        await sendMut.mutateAsync({ sid: sessionId, text });
      } catch {
        setLoading(false);
      }
    },
    [sessionId, sendMut],
  );

  const sendInput = useCallback(
    async () => {
      if (!input.trim() || loading || sessionId === null) return;
      setLoading(true);
      setInput("");
      try {
        await sendMut.mutateAsync({ sid: sessionId, text: input.trim() });
      } catch {
        setLoading(false);
      }
    },
    [input, loading, sessionId, sendMut],
  );

  return (
    <Dialog open={open} title="管家助手" onClose={onClose} className="max-w-2xl">
      <div className="flex flex-col" style={{ height: "400px" }}>
        {initError ? (
          <div className="flex flex-1 items-center justify-center text-sm text-ink-muted">
            管家暂时不可用，请稍后再试
          </div>
        ) : sessionId === null ? (
          <div className="flex flex-1 items-center justify-center text-sm text-ink-muted">
            正在连接管家...
          </div>
        ) : (
          <>
            <div
              ref={scrollRef}
              className="min-h-0 flex-1 overflow-y-auto space-y-2 pr-1"
              data-testid="steward-dialog-messages"
            >
              {messages.length === 0 && !loading && (
                <div className="flex h-full items-center justify-center text-sm text-ink-muted">
                  正在等待管家回复...
                </div>
              )}
              {messages.map((msg, i) => (
                <div
                  key={i}
                  className={
                    msg.role === "user"
                      ? "flex justify-end"
                      : "flex justify-start"
                  }
                >
                  <div
                    className={
                      msg.role === "user"
                        ? "max-w-[80%] rounded bg-surface-overlay px-3 py-2 text-sm text-ink"
                        : "max-w-[80%] whitespace-pre-wrap rounded bg-surface-raised px-3 py-2 text-sm text-ink"
                    }
                  >
                    {msg.text}
                  </div>
                </div>
              ))}
              {loading && (
                <div className="flex justify-start">
                  <div className="rounded bg-surface-raised px-3 py-2 text-sm text-ink-muted">
                    <span className="animate-pulse">思考中...</span>
                  </div>
                </div>
              )}
            </div>
            <div className="flex shrink-0 items-center gap-2 border-t border-ink-muted/20 pt-2">
              <input
                type="text"
                value={input}
                onChange={(e) => setInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    void sendInput();
                  }
                }}
                placeholder="输入消息..."
                className="min-w-0 flex-1 rounded border border-ink-muted/30 bg-surface-raised px-2 py-1 text-xs text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
                data-testid="steward-dialog-input"
              />
              <button
                type="button"
                onClick={() => void sendInput()}
                disabled={loading || !input.trim()}
                className="shrink-0 rounded bg-ink-accent px-3 py-1 text-xs text-surface disabled:opacity-50"
                data-testid="steward-dialog-send"
              >
                <Icon name="send" size={14} />
              </button>
            </div>
          </>
        )}
      </div>
    </Dialog>
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
