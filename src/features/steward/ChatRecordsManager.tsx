import { useState } from "react";
import type { ReactNode } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { ClearConfirmDialog } from "./ClearConfirmDialog";
import type { MessageStatsDto, StewardMessageDto } from "../../lib/ipc/bindings.gen";

export interface ChatRecordsManagerProps {
  sessionId: string;
  messageStats: MessageStatsDto;
  messages: StewardMessageDto[];
}

export function ChatRecordsManager({
  sessionId,
  messageStats,
  messages,
}: ChatRecordsManagerProps): ReactNode {
  const qc = useQueryClient();
  const [showClearDialog, setShowClearDialog] = useState(false);
  const [selectMode, setSelectMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());

  const clearMut = useMutation({
    mutationFn: () => ipc.stewardClearMessages(sessionId),
    onSuccess: () => {
      toast.success("聊天记录已清空");
      void qc.invalidateQueries({ queryKey: ["steward"] });
    },
    onError: (e) => toast.error(`清空失败: ${e instanceof Error ? e.message : String(e)}`),
  });

  const deleteMut = useMutation({
    mutationFn: (ids: string[]) => ipc.stewardBatchDeleteMessages(sessionId, ids),
    onSuccess: () => {
      toast.success("消息已删除");
      setSelectedIds(new Set());
      setSelectMode(false);
      void qc.invalidateQueries({ queryKey: ["steward"] });
    },
    onError: (e) => toast.error(`删除失败: ${e instanceof Error ? e.message : String(e)}`),
  });

  const hasMessages = messageStats.total > 0;

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-ink">聊天记录</h3>
        <span className="text-xs text-ink-muted">{messageStats.total} 条消息</span>
      </div>

      {!hasMessages && (
        <p className="text-xs text-ink-muted">暂无消息</p>
      )}

      {hasMessages && !selectMode && (
        <div className="flex gap-2">
          <button
            type="button"
            className="sketch-btn px-2 py-0.5 text-xs text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
            onClick={() => setSelectMode(true)}
          >
            批量删除
          </button>
          <button
            type="button"
            className="rounded border border-state-danger px-2 py-0.5 text-xs text-state-danger hover:bg-surface focus-visible:ring-2 focus-visible:ring-ink-accent"
            onClick={() => setShowClearDialog(true)}
          >
            清空记录
          </button>
        </div>
      )}

      {selectMode && (
        <div className="space-y-2">
          <div className="sketch-panel max-h-40 overflow-y-auto p-1">
            {messages.map((msg) => (
              <label
                key={msg.id}
                className="flex cursor-pointer items-center gap-2 rounded p-1 text-xs hover:bg-surface-overlay"
              >
                <input
                  type="checkbox"
                  checked={selectedIds.has(msg.id)}
                  onChange={(e) => {
                    setSelectedIds((prev) => {
                      const next = new Set(prev);
                      if (e.target.checked) next.add(msg.id);
                      else next.delete(msg.id);
                      return next;
                    });
                  }}
                />
                <span className={msg.role === "user" ? "text-ink" : "text-ink-muted"}>
                  {msg.role === "user" ? "我" : "管家"}: {msg.text.slice(0, 40)}
                </span>
              </label>
            ))}
          </div>
          <div className="flex gap-2">
            <button
              type="button"
              className="rounded bg-state-danger px-2 py-0.5 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
              onClick={() => deleteMut.mutate([...selectedIds])}
              disabled={selectedIds.size === 0 || deleteMut.isPending}
            >
              {deleteMut.isPending ? "删除中..." : `删除选中（${selectedIds.size}）`}
            </button>
            <button
              type="button"
              className="sketch-btn px-2 py-0.5 text-xs text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
              onClick={() => {
                setSelectMode(false);
                setSelectedIds(new Set());
              }}
            >
              取消
            </button>
          </div>
        </div>
      )}

      <ClearConfirmDialog
        open={showClearDialog}
        title="清空聊天记录"
        description={`确定要清空全部 ${messageStats.total} 条聊天记录吗？管家配置与身份不受影响。`}
        onConfirm={() => clearMut.mutate()}
        onClose={() => setShowClearDialog(false)}
      />
    </div>
  );
}
