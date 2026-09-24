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

interface SplitViewProps {
  renderWorkspace: (workspaceId: string) => React.ReactNode;
}

export function SplitView({ renderWorkspace }: SplitViewProps) {
  const { splitWorkspaceIds, focusWorkspace, setLayoutMode, setSplitWorkspaceIds } =
    useUiStore();

  const { data: workspaces } = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => invoke<WorkspaceEntryDto[]>("list_workspaces"),
  });

  if (!splitWorkspaceIds) return null;
  const [leftId, rightId] = splitWorkspaceIds;

  const wsMap = new Map((workspaces ?? []).map((w) => [w.id, w]));
  const leftWs = wsMap.get(leftId);
  const rightWs = wsMap.get(rightId);

  const renderPane = (id: string, ws: WorkspaceEntryDto | undefined) => {
    if (!ws || !ws.directoryPresent) {
      return (
        <div className="flex h-full items-center justify-center text-sm text-paper-muted">
          <div className="text-center">
            <p className="mb-2">目录缺失</p>
            <p className="text-xs text-paper-faint">{ws?.rootPath ?? id}</p>
          </div>
        </div>
      );
    }
    return renderWorkspace(id);
  };

  return (
    <div className="flex h-full w-full gap-px bg-paper-line/30">
      <div
        className="flex-1 overflow-hidden bg-paper-bg relative group"
        onClick={() => void focusWorkspace(leftId)}
      >
        {renderPane(leftId, leftWs)}
        <button
          onClick={(e) => {
            e.stopPropagation();
            void setLayoutMode("single");
            void setSplitWorkspaceIds(null);
            void focusWorkspace(leftId);
          }}
          className="absolute top-1 right-1 px-2 py-0.5 text-xs rounded bg-paper-bg/80 text-paper-muted opacity-0 group-hover:opacity-100 hover:text-paper-fg"
        >
          保留此侧
        </button>
      </div>
      <div
        className="flex-1 overflow-hidden bg-paper-bg relative group"
        onClick={() => void focusWorkspace(rightId)}
      >
        {renderPane(rightId, rightWs)}
        <button
          onClick={(e) => {
            e.stopPropagation();
            void setLayoutMode("single");
            void setSplitWorkspaceIds(null);
            void focusWorkspace(rightId);
          }}
          className="absolute top-1 right-1 px-2 py-0.5 text-xs rounded bg-paper-bg/80 text-paper-muted opacity-0 group-hover:opacity-100 hover:text-paper-fg"
        >
          保留此侧
        </button>
      </div>
    </div>
  );
}
