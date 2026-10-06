import type { ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { useStewardSessions } from "../../lib/store/useStewardSessions";
import { toast } from "../../lib/store/toastStore";
import { ConfigConfirm } from "./ConfigConfirm";

export function StewardChat(): ReactNode {
  const qc = useQueryClient();
  const { sessions, currentSessionId, setSessions, setCurrentSessionId, addSession } =
    useStewardSessions();

  const sessionsQuery = useQuery({
    queryKey: ["steward", "sessions"],
    queryFn: () => ipc.stewardListSessions(),
    staleTime: 5000,
  });

  if (sessionsQuery.data && sessionsQuery.data !== sessions) {
    setSessions(sessionsQuery.data);
  }

  const createMut = useMutation({
    mutationFn: (title: string) => ipc.stewardCreateSession(title),
    onSuccess: (session) => {
      addSession(session);
      void qc.invalidateQueries({ queryKey: ["steward", "sessions"] });
    },
    onError: (e) => toast.error(`创建会话失败: ${e}`),
  });

  const sendMut = useMutation({
    mutationFn: ({ sessionId, text }: { sessionId: string; text: string }) =>
      ipc.stewardSendMessage(sessionId, text),
    onError: (e) => toast.error(`发送失败: ${e}`),
  });

  return (
    <div className="flex h-full">
      <div className="w-64 border-r border-zinc-800 overflow-y-auto p-2">
        <button
          className="w-full rounded bg-zinc-800 px-3 py-1.5 text-sm hover:bg-zinc-700"
          onClick={() => createMut.mutate("新会话")}
        >
          + 新建会话
        </button>
        <ul className="mt-2 space-y-1">
          {sessions.map((s) => (
            <li key={s.id}>
              <button
                className={`w-full rounded px-3 py-1.5 text-left text-sm ${
                  currentSessionId === s.id
                    ? "bg-zinc-800 text-zinc-100"
                    : "text-zinc-400 hover:bg-zinc-900"
                }`}
                onClick={() => setCurrentSessionId(s.id)}
              >
                {s.title}
              </button>
            </li>
          ))}
        </ul>
      </div>
      <div className="flex-1 flex flex-col">
        {currentSessionId ? (
          <StewardChatMain
            onSend={(text) => sendMut.mutate({ sessionId: currentSessionId, text })}
            sending={sendMut.isPending}
            reply={sendMut.data}
          />
        ) : (
          <div className="flex-1 flex items-center justify-center text-zinc-500">
            选择或创建一个会话
          </div>
        )}
      </div>
    </div>
  );
}

function StewardChatMain({
  onSend,
  sending,
  reply,
}: {
  onSend: (text: string) => void;
  sending: boolean;
  reply: Awaited<ReturnType<typeof ipc.stewardSendMessage>> | undefined;
}): ReactNode {
  let input = "";
  return (
    <div className="flex-1 flex flex-col">
      <div className="flex-1 overflow-y-auto p-4">
        {reply && <StewardReplyView reply={reply} />}
      </div>
      <div className="border-t border-zinc-800 p-3">
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (input.trim()) {
              onSend(input.trim());
              input = "";
            }
          }}
          className="flex gap-2"
        >
          <input
            className="flex-1 rounded bg-zinc-900 px-3 py-1.5 text-sm text-zinc-100 outline-none border border-zinc-800 focus:border-zinc-600"
            placeholder="向管家发送消息..."
            onChange={(e) => {
              input = e.target.value;
            }}
            disabled={sending}
          />
          <button
            type="submit"
            className="rounded bg-zinc-700 px-4 py-1.5 text-sm hover:bg-zinc-600 disabled:opacity-50"
            disabled={sending}
          >
            发送
          </button>
        </form>
      </div>
    </div>
  );
}

function StewardReplyView({
  reply,
}: {
  reply: Awaited<ReturnType<typeof ipc.stewardSendMessage>>;
}): ReactNode {
  switch (reply.type) {
    case "text":
      return <div className="rounded bg-zinc-900 p-3 text-sm text-zinc-200">{reply.content}</div>;
    case "config_proposal":
      return <ConfigConfirm proposalId={reply.proposal_id} diff={reply.diff} />;
    case "evolution_accepted":
      return (
        <div className="rounded bg-zinc-900 p-3 text-sm text-zinc-200">
          进化周期已受理，周期 ID: {reply.cycle_id}
        </div>
      );
    case "clarify":
      return (
        <div className="rounded bg-zinc-900 p-3 text-sm text-zinc-200">
          <p>请明确您的意图：</p>
          <ul className="mt-2 list-disc pl-5">
            {reply.candidates.map((c, i) => (
              <li key={i}>{c.kind}</li>
            ))}
          </ul>
        </div>
      );
    case "refused":
      return (
        <div className="rounded bg-red-950 p-3 text-sm text-red-200">
          拒绝: {reply.reason}
        </div>
      );
  }
}
