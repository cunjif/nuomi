import type { ReactNode } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { useStewardEvents } from "../../lib/stewardHooks";

export function SettingsView(): ReactNode {
  const qc = useQueryClient();
  const eventsQuery = useStewardEvents(null, null, 50);

  const authMut = useMutation({
    mutationFn: (authorized: boolean) =>
      ipc.stewardSetOnlineAuthorization(authorized),
    onSuccess: () => {
      toast.success("授权已更新");
      void qc.invalidateQueries({ queryKey: ["steward"] });
    },
    onError: (e) => toast.error(`更新失败: ${e}`),
  });

  return (
    <div className="h-full overflow-y-auto p-4">
      <h2 className="mb-3 text-sm font-semibold">管家设置</h2>

      <section className="mb-4">
        <h3 className="mb-2 text-xs font-semibold text-zinc-400">联网授权</h3>
        <div className="flex gap-2">
          <button
            className="rounded bg-green-800 px-3 py-1 text-xs hover:bg-green-700"
            onClick={() => authMut.mutate(true)}
          >
            授权联网
          </button>
          <button
            className="rounded bg-red-800 px-3 py-1 text-xs hover:bg-red-700"
            onClick={() => authMut.mutate(false)}
          >
            取消授权
          </button>
        </div>
      </section>

      <section>
        <h3 className="mb-2 text-xs font-semibold text-zinc-400">事件流</h3>
        <ul className="space-y-1">
          {(eventsQuery.data ?? []).map((evt) => (
            <li
              key={evt.id}
              className="rounded bg-zinc-900 p-2 text-xs text-zinc-400"
            >
              <span className="text-zinc-300">{evt.kind}</span>
              <span className="ml-2 text-zinc-600">{evt.aggregateId}</span>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}
