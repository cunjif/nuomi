import type { ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { useEvolutionCycles } from "../../lib/store/useEvolutionCycles";

const PHASES = [
  "cleanse",
  "research",
  "design",
  "develop",
  "test",
  "verify",
  "gate",
  "merge",
] as const;

export function CycleProgress(): ReactNode {
  const qc = useQueryClient();
  const { cycles, setDetail } = useEvolutionCycles();

  const cyclesQuery = useQuery({
    queryKey: ["steward", "cycles"],
    queryFn: () => ipc.stewardListCycles(null),
    staleTime: 5000,
  });

  if (cyclesQuery.data) {
    cyclesQuery.data.forEach((c) => {
      void ipc.stewardGetCycle(c.id).then((detail) => setDetail(c.id, detail));
    });
  }

  const cancelMut = useMutation({
    mutationFn: (cycleId: string) => ipc.stewardCancelCycle(cycleId),
    onSuccess: () => {
      toast.success("周期已取消");
      void qc.invalidateQueries({ queryKey: ["steward", "cycles"] });
    },
    onError: (e) => toast.error(`取消失败: ${e}`),
  });

  return (
    <div className="h-full overflow-y-auto p-4">
      <h2 className="mb-3 text-sm font-semibold">进化周期</h2>
      <ul className="space-y-3">
        {(cyclesQuery.data ?? cycles).map((cycle) => {
          const currentPhaseIdx = PHASES.indexOf(
            cycle.phase as (typeof PHASES)[number],
          );
          return (
            <li key={cycle.id} className="rounded bg-zinc-900 p-3">
              <div className="mb-2 flex items-center justify-between">
                <span className="text-sm text-zinc-300">
                  {cycle.triggerSource}: {cycle.triggerContext}
                </span>
                <span
                  className={`text-xs ${
                    cycle.status === "running"
                      ? "text-green-400"
                      : cycle.status === "cancelled"
                        ? "text-red-400"
                        : "text-zinc-500"
                  }`}
                >
                  {cycle.status}
                </span>
              </div>
              <div className="mb-2 flex gap-1">
                {PHASES.map((phase, idx) => (
                  <div
                    key={phase}
                    className={`h-1.5 flex-1 rounded ${
                      idx <= currentPhaseIdx ? "bg-green-600" : "bg-zinc-700"
                    }`}
                  />
                ))}
              </div>
              {cycle.status === "running" && (
                <button
                  className="rounded bg-red-800 px-2 py-1 text-xs hover:bg-red-700"
                  onClick={() => cancelMut.mutate(cycle.id)}
                >
                  取消
                </button>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
