import { useUiStore } from "../../lib/store/uiStore";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";

interface RecentWorkspaceDto {
  workspaceId: string;
  lastUsedAt: number;
  isPinned: boolean;
}

export function EmptyStateGuide() {
  const { openWorkspace } = useUiStore();

  const { data: recentWorkspaces } = useQuery({
    queryKey: ["recent-workspaces"],
    queryFn: () => invoke<RecentWorkspaceDto[]>("get_recent_workspaces", { limit: 10 }),
  });

  const handleOpenDirectory = async () => {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({ directory: true });
    if (typeof selected === "string") {
      const entry = await invoke<{ id: string }>("add_workspace", { path: selected });
      openWorkspace(entry.id);
    }
  };

  return (
    <div className="flex flex-col items-center justify-center h-full gap-6 p-8">
      <div className="text-center">
        <h2 className="text-xl font-medium text-paper-fg mb-2">没有已开启的工作区</h2>
        <p className="text-sm text-paper-muted">
          打开一个目录开始工作，或从最近使用列表中恢复
        </p>
      </div>
      <button
        onClick={handleOpenDirectory}
        className="px-4 py-2 rounded-md bg-paper-accent text-paper-bg text-sm hover:opacity-90"
      >
        打开目录
      </button>
      {recentWorkspaces && recentWorkspaces.length > 0 && (
        <div className="w-full max-w-md">
          <h3 className="text-sm font-medium text-paper-fg mb-2">最近使用</h3>
          <div className="flex flex-col gap-1">
            {recentWorkspaces.map((ws) => (
              <button
                key={ws.workspaceId}
                onClick={() => openWorkspace(ws.workspaceId)}
                className="flex items-center gap-2 px-3 py-2 rounded-md hover:bg-paper-bg/60 text-sm text-left"
              >
                {ws.isPinned && <span className="text-xs text-paper-accent">★</span>}
                <span className="flex-1 truncate">{ws.workspaceId}</span>
                <span className="text-xs text-paper-faint">
                  {new Date(ws.lastUsedAt).toLocaleDateString()}
                </span>
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
