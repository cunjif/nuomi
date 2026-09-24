import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { measureAsync } from "../../lib/perf/metrics";

interface CrossSearchOutcomeDto {
  groups: {
    workspaceId: string;
    workspaceName: string;
    matches: { relativePath: string; matchType: string }[];
  }[];
  skippedWorkspaceIds: string[];
}

export function CrossSearchPanel() {
  const [query, setQuery] = useState("");
  const [matchContent, setMatchContent] = useState(false);
  const [result, setResult] = useState<CrossSearchOutcomeDto | null>(null);
  const [searching, setSearching] = useState(false);

  const search = useCallback(async () => {
    if (!query.trim()) return;
    setSearching(true);
    try {
      const outcome = await measureAsync(
        "workspace.crossSearch",
        () => invoke<CrossSearchOutcomeDto>("cross_workspace_search", { query, matchContent }),
        { query, matchContent },
      );
      setResult(outcome);
    } finally {
      setSearching(false);
    }
  }, [query, matchContent]);

  return (
    <div className="flex flex-col h-full bg-paper-bg">
      <div className="flex items-center gap-2 p-3 border-b border-paper-line/30">
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && search()}
          placeholder="跨工作区搜索..."
          className="flex-1 px-3 py-1.5 rounded-md border border-paper-line/40 bg-paper-bg text-sm focus:outline-none focus:border-paper-accent"
        />
        <label className="flex items-center gap-1 text-xs text-paper-muted">
          <input
            type="checkbox"
            checked={matchContent}
            onChange={(e) => setMatchContent(e.target.checked)}
          />
          内容
        </label>
        <button
          onClick={search}
          disabled={searching}
          className="px-3 py-1.5 rounded-md bg-paper-accent text-paper-bg text-sm disabled:opacity-50"
        >
          {searching ? "搜索中..." : "搜索"}
        </button>
      </div>
      <div className="flex-1 overflow-auto p-3">
        {result && result.skippedWorkspaceIds.length > 0 && (
          <div className="mb-3 p-2 rounded-md bg-paper-warn/10 text-xs text-paper-warn">
            已跳过 {result.skippedWorkspaceIds.length} 个目录缺失的工作区
          </div>
        )}
        {result?.groups.map((group) => (
          <div key={group.workspaceId} className="mb-4">
            <div className="flex items-center gap-2 mb-1 text-sm font-medium">
              <span className="text-paper-muted">📁</span>
              {group.workspaceName}
            </div>
            {group.matches.map((match, i) => (
              <div
                key={i}
                className="ml-6 py-0.5 text-sm text-paper-muted hover:text-paper-fg cursor-pointer"
              >
                {match.relativePath}
                <span className="ml-2 text-xs text-paper-faint">
                  {match.matchType === "fileName" ? "文件名" : "内容"}
                </span>
              </div>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
