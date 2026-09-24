import { useUiStore } from "../../lib/store/uiStore";
import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";

interface WorkspaceEntryDto {
  id: string;
  rootPath: string;
  colorTag: string;
  createdAt: number;
  isActive: boolean;
  directoryPresent: boolean;
}

export function OverviewGrid() {
  const { openWorkspaceIds, focusWorkspace, setLayoutMode } = useUiStore();

  const { data: workspaces } = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => invoke<WorkspaceEntryDto[]>("list_workspaces"),
  });

  const openWorkspaces = (workspaces ?? []).filter((w) =>
    openWorkspaceIds.includes(w.id),
  );

  if (openWorkspaces.length === 0) return null;

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 px-6 py-2 text-xs text-paper-muted bg-paper-surface/50 border-b border-paper-line/30">
        <span>概览模式 · 仅用于监控与跳转，点击卡片进入单个工作区以启动 Agent</span>
      </div>
      <div className="grid grid-cols-2 lg:grid-cols-3 gap-4 p-6 flex-1 overflow-auto">
        {openWorkspaces.map((ws) => (
          <button
            key={ws.id}
            onClick={() => {
              focusWorkspace(ws.id);
              setLayoutMode("single");
            }}
            className="flex flex-col gap-2 p-4 rounded-lg border border-paper-line/40 bg-paper-bg hover:border-paper-accent/60 transition-colors text-left"
          >
            <div className="flex items-center gap-2">
              <span
                className="w-3 h-3 rounded-full shrink-0"
                style={{ backgroundColor: `var(--color-${ws.colorTag})` }}
              />
              <span className="font-medium truncate">
                {ws.rootPath.split(/[/\\]/).pop()}
              </span>
            </div>
            <span className="text-xs text-paper-muted truncate">{ws.rootPath}</span>
            <div className="flex items-center gap-3 text-xs text-paper-muted">
              <span>{ws.directoryPresent ? "✓ 可访问" : "✗ 目录缺失"}</span>
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}
