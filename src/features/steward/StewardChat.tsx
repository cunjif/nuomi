import { useState, useCallback } from "react";
import type { ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import type { StewardReplyDto } from "../../lib/ipc/bindings.gen";

interface ChatMessage {
  role: "user" | "steward";
  text: string;
  reply?: StewardReplyDto;
}

export function StewardChat(): ReactNode {
  const qc = useQueryClient();
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [messages, setMessages] = useState<ChatMessage[]>([]);

  const sessionsQuery = useQuery({
    queryKey: ["steward", "sessions"],
    queryFn: () => ipc.stewardListSessions(),
    staleTime: 5000,
  });

  const createMut = useMutation({
    mutationFn: (title: string) => ipc.stewardCreateSession(title),
    onSuccess: (session) => {
      void qc.invalidateQueries({ queryKey: ["steward", "sessions"] });
      setCurrentSessionId(session.id);
      setMessages([]);
    },
    onError: (e) => toast.error(`创建会话失败: ${e}`),
  });

  const sendMut = useMutation({
    mutationFn: ({ sessionId, text }: { sessionId: string; text: string }) =>
      ipc.stewardSendMessage(sessionId, text),
    onSuccess: (reply, vars) => {
      setMessages((prev) => [
        ...prev,
        { role: "user", text: vars.text },
        { role: "steward", text: replyText(reply), reply },
      ]);
    },
    onError: (e, vars) => {
      setMessages((prev) => [
        ...prev,
        { role: "user", text: vars.text },
        { role: "steward", text: `发送失败: ${e}` },
      ]);
    },
  });

  const handleSend = useCallback(() => {
    const trimmed = input.trim();
    if (!trimmed || !currentSessionId || sendMut.isPending) return;
    setInput("");
    sendMut.mutate({ sessionId: currentSessionId, text: trimmed });
  }, [input, currentSessionId, sendMut]);

  return (
    <div className="flex h-full">
      <div className="w-56 shrink-0 overflow-y-auto border-r border-ink-muted/30 p-2">
        <button
          className="w-full rounded-[12px_255px_15px_225px/225px_15px_255px_12px] bg-surface-overlay px-3 py-1.5 text-left font-note-hand text-sm hover:bg-surface-raised"
          onClick={() => createMut.mutate("新管家会话")}
        >
          + 新建会话
        </button>
        <ul className="mt-2 space-y-1">
          {(sessionsQuery.data ?? []).map((s) => (
            <li key={s.id}>
              <button
                className={`w-full rounded-[12px_255px_15px_225px/225px_15px_255px_12px] px-3 py-1.5 text-left font-note-hand text-sm ${
                  currentSessionId === s.id
                    ? "bg-surface-overlay text-ink-accent"
                    : "text-ink-muted hover:bg-surface-overlay"
                }`}
                onClick={() => {
                  setCurrentSessionId(s.id);
                  setMessages([]);
                }}
              >
                {s.title}
              </button>
            </li>
          ))}
        </ul>
      </div>
      <div className="flex min-h-0 flex-1 flex-col">
        {currentSessionId ? (
          <>
            <div className="min-h-0 flex-1 overflow-y-auto p-4">
              {messages.length === 0 ? (
                <div className="flex h-full items-center justify-center text-sm text-ink-muted">
                  向管家发送消息，例如"诊断应用状态"、"触发进化"等
                </div>
              ) : (
                <div className="space-y-3">
                  {messages.map((msg, i) => (
                    <MessageBubble key={i} msg={msg} />
                  ))}
                </div>
              )}
            </div>
            <div className="shrink-0 border-t border-ink-muted/30 p-3">
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  handleSend();
                }}
                className="flex gap-2"
              >
                <input
                  className="flex-1 rounded border border-ink-muted/30 bg-surface px-3 py-1.5 text-sm text-ink outline-none focus:border-ink-accent"
                  placeholder="向管家发送消息..."
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  disabled={sendMut.isPending}
                />
                <button
                  type="submit"
                  className="rounded bg-ink-accent px-4 py-1.5 text-sm text-surface disabled:opacity-50"
                  disabled={sendMut.isPending || !input.trim()}
                >
                  发送
                </button>
              </form>
            </div>
          </>
        ) : (
          <div className="flex flex-1 items-center justify-center text-sm text-ink-muted">
            选择或创建一个管家会话
          </div>
        )}
      </div>
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

function MessageBubble({ msg }: { msg: ChatMessage }): ReactNode {
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
  if (reply && reply.type === "config_proposal") {
    return <ConfigProposalBubble reply={reply} />;
  }  return (
    <div className="flex justify-start">
      <div className="max-w-[80%] whitespace-pre-wrap rounded bg-surface-raised px-3 py-2 text-sm text-ink">
        {msg.text}
      </div>
    </div>
  );
}

function ConfigProposalBubble({ reply }: { reply: Extract<StewardReplyDto, { type: "config_proposal" }> }): ReactNode {
  const qc = useQueryClient();
  const confirmMut = useMutation({
    mutationFn: () => ipc.stewardConfirmConfigChange(reply.proposal_id),
    onSuccess: () => {
      toast.success("配置变更已确认");
      void qc.invalidateQueries({ queryKey: ["steward"] });
    },
    onError: (e) => toast.error(`确认失败: ${e}`),
  });
  return (
    <div className="flex justify-start">
      <div className="max-w-[80%] rounded bg-surface-raised p-3">
        <p className="mb-2 text-sm font-medium text-ink">配置变更建议</p>
        <pre className="mb-3 max-h-60 overflow-auto rounded bg-surface px-2 py-1 text-xs text-ink-muted">
          {reply.diff}
        </pre>
        <button
          className="rounded bg-ink-accent px-3 py-1 text-xs text-surface disabled:opacity-50"
          onClick={() => confirmMut.mutate()}
          disabled={confirmMut.isPending}
        >
          {confirmMut.isPending ? "确认中..." : "确认变更"}
        </button>
      </div>
    </div>
  );
}
