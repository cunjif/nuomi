import type { ReactNode } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

export function ConfigConfirm({
  proposalId,
  diff,
}: {
  proposalId: string;
  diff: string;
}): ReactNode {
  const qc = useQueryClient();

  const confirmMut = useMutation({
    mutationFn: () => ipc.stewardConfirmConfigChange(proposalId),
    onSuccess: () => {
      toast.success("配置变更已确认");
      void qc.invalidateQueries({ queryKey: ["steward"] });
    },
    onError: (e) => toast.error(`确认失败: ${e}`),
  });

  const rollbackMut = useMutation({
    mutationFn: (snapshotId: string) => ipc.stewardRollbackConfigChange(snapshotId),
    onSuccess: () => {
      toast.success("配置已回滚");
      void qc.invalidateQueries({ queryKey: ["steward"] });
    },
    onError: (e) => toast.error(`回滚失败: ${e}`),
  });

  return (
    <div className="rounded bg-zinc-900 p-3">
      <h3 className="mb-2 text-sm font-semibold text-zinc-200">配置变更建议</h3>
      <pre className="mb-3 max-h-60 overflow-auto rounded bg-zinc-950 p-2 text-xs text-zinc-300">
        {diff}
      </pre>
      <div className="flex gap-2">
        <button
          className="rounded bg-green-800 px-3 py-1 text-xs hover:bg-green-700"
          onClick={() => confirmMut.mutate()}
          disabled={confirmMut.isPending}
        >
          确认
        </button>
        <button
          className="rounded bg-zinc-700 px-3 py-1 text-xs hover:bg-zinc-600"
          onClick={() => {
            if (confirmMut.data?.snapshotId) {
              rollbackMut.mutate(confirmMut.data.snapshotId);
            }
          }}
          disabled={!confirmMut.data?.snapshotId || rollbackMut.isPending}
        >
          回滚
        </button>
      </div>
    </div>
  );
}
