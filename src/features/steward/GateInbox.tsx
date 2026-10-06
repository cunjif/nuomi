import type { ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { useGatePending } from "../../lib/store/useGatePending";

export function GateInbox(): ReactNode {
  const qc = useQueryClient();
  const { setPending, removeArtifact } = useGatePending();

  const pendingQuery = useQuery({
    queryKey: ["steward", "gatePending"],
    queryFn: () => ipc.stewardListGatePending(),
    staleTime: 3000,
  });

  if (pendingQuery.data) {
    setPending(pendingQuery.data);
  }

  const resolveMut = useMutation({
    mutationFn: ({
      artifactId,
      decision,
    }: {
      artifactId: string;
      decision: { kind: string; feedback?: string };
    }) => ipc.stewardResolveGate(artifactId, decision),
    onSuccess: (_data, vars) => {
      removeArtifact(vars.artifactId);
      void qc.invalidateQueries({ queryKey: ["steward", "gatePending"] });
    },
    onError: (e) => toast.error(`决策失败: ${e}`),
  });

  return (
    <div className="h-full overflow-y-auto p-4">
      <h2 className="mb-3 text-sm font-semibold">验收门收件箱</h2>
      <ul className="space-y-2">
        {(pendingQuery.data ?? []).map((artifact) => (
          <li key={artifact.id} className="rounded bg-zinc-900 p-3">
            <div className="mb-2 flex items-center justify-between">
              <span className="text-sm text-zinc-300">{artifact.artifactType}</span>
              <span className="text-xs text-zinc-500">{artifact.producedByRole}</span>
            </div>
            {artifact.diffPreview && (
              <pre className="mb-2 max-h-40 overflow-auto rounded bg-zinc-950 p-2 text-xs text-zinc-400">
                {artifact.diffPreview}
              </pre>
            )}
            <div className="flex gap-2">
              <button
                className="rounded bg-green-800 px-2 py-1 text-xs hover:bg-green-700"
                onClick={() =>
                  resolveMut.mutate({
                    artifactId: artifact.id,
                    decision: { kind: "approve" },
                  })
                }
              >
                批准
              </button>
              <button
                className="rounded bg-red-800 px-2 py-1 text-xs hover:bg-red-700"
                onClick={() =>
                  resolveMut.mutate({
                    artifactId: artifact.id,
                    decision: { kind: "reject" },
                  })
                }
              >
                驳回
              </button>
              <button
                className="rounded bg-zinc-700 px-2 py-1 text-xs hover:bg-zinc-600"
                onClick={() => {
                  const feedback = window.prompt("修改反馈:");
                  if (feedback) {
                    resolveMut.mutate({
                      artifactId: artifact.id,
                      decision: { kind: "request_changes", feedback },
                    });
                  }
                }}
              >
                要求修改
              </button>
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
