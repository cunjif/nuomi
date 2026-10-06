import type { ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";

const DEV_ROLES = [
  { kind: "researcher", label: "调研" },
  { kind: "designer", label: "设计" },
  { kind: "developer", label: "开发" },
  { kind: "tester", label: "测试" },
  { kind: "verifier", label: "验收" },
] as const;

export function TeamManagement(): ReactNode {
  const qc = useQueryClient();
  const teamQuery = useQuery({
    queryKey: ["steward", "devTeam"],
    queryFn: () => ipc.stewardGetDevTeam(),
    staleTime: 10_000,
  });

  const bindingMut = useMutation({
    mutationFn: (binding: Parameters<typeof ipc.stewardSetDevRoleBinding>[0]) =>
      ipc.stewardSetDevRoleBinding(binding),
    onSuccess: () => {
      toast.success("绑定已更新");
      void qc.invalidateQueries({ queryKey: ["steward", "devTeam"] });
    },
    onError: (e) => toast.error(`更新失败: ${e}`),
  });

  if (!teamQuery.data) {
    return <div className="p-4 text-sm text-zinc-500">加载中...</div>;
  }

  return (
    <div className="h-full overflow-y-auto p-4">
      <h2 className="mb-3 text-sm font-semibold">研发团队管理</h2>
      <ul className="space-y-2">
        {DEV_ROLES.map((role) => {
          const binding = teamQuery.data!.bindings.find(
            (b) => b.roleKind === role.kind,
          );
          return (
            <li
              key={role.kind}
              className="flex items-center gap-3 rounded bg-zinc-900 p-3"
            >
              <span className="w-16 text-sm text-zinc-300">{role.label}</span>
              <span className="flex-1 text-xs text-zinc-500">
                {binding
                  ? `${binding.agentKind}: ${binding.agentRefId}`
                  : "未绑定"}
              </span>
              <button
                className="rounded bg-zinc-700 px-2 py-1 text-xs hover:bg-zinc-600"
                onClick={() => {
                  const refId = window.prompt("输入 Agent 或 Provider ID:");
                  if (refId) {
                    bindingMut.mutate({
                      roleKind: role.kind,
                      agentKind: "cli",
                      agentRefId: refId,
                    });
                  }
                }}
              >
                替换绑定
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
