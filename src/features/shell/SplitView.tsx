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
  renderWorkspace: (workspaceId: string, paneIndex: number) => React.ReactNode;
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

  const renderPane = (id: string, ws: WorkspaceEntryDto | undefined, paneIndex: number) => {
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
    return renderWorkspace(id, paneIndex);
  };

  const renderPaneHeader = (id: string, ws: WorkspaceEntryDto | undefined, side: "left" | "right"): React.ReactNode => {
    const name = ws ? (ws.rootPath.split(/[/\\]/).pop() ?? id) : id;
    return (
      <div className="flex shrink-0 items-center gap-2 border-b border-paper-line/30 bg-paper-surface/50 px-2 py-1 text-xs">
        <span
          className="h-2.5 w-2.5 shrink-0 rounded-full"
          style={{ backgroundColor: ws ? `var(--color-${ws.colorTag})` : "var(--paper-muted)" }}
          aria-hidden="true"
        />
        <span className="truncate font-medium text-paper-fg">{name}</span>
        {ws && !ws.directoryPresent && <span className="text-state-warn">!</span>}
        <button
          onClick={(e) => {
            e.stopPropagation();
            void setLayoutMode("single");
            void setSplitWorkspaceIds(null);
            void focusWorkspace(id);
          }}
          className="ml-auto px-1.5 py-0.5 text-xs rounded bg-paper-bg/80 text-paper-muted hover:text-paper-fg"
        >
          {side === "left" ? "保留左侧" : "保留右侧"}
        </button>
      </div>
    );
  };

  return (
    <div className="flex h-full w-full gap-px bg-paper-line/30">
      <div
        className="flex flex-1 flex-col overflow-hidden bg-paper-bg"
        onClick={() => void focusWorkspace(leftId)}
      >
        {renderPaneHeader(leftId, leftWs, "left")}
        <div className="min-h-0 flex-1">{renderPane(leftId, leftWs, 0)}</div>
      </div>
      <div
        className="flex flex-1 flex-col overflow-hidden bg-paper-bg"
        onClick={() => void focusWorkspace(rightId)}
      >
        {renderPaneHeader(rightId, rightWs, "right")}
        <div className="min-h-0 flex-1">{renderPane(rightId, rightWs, 1)}</div>
      </div>
    </div>
  );
}
