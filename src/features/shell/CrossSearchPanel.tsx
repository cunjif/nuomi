import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { measureAsync } from "../../lib/perf/metrics";
import { useUiStore } from "../../lib/store/uiStore";

interface CrossSearchOutcomeDto {
  groups: {
    workspaceId: string;
    workspaceName: string;
    matches: { relativePath: string; matchType: string }[];
  }[];
  skippedWorkspaceIds: string[];
}

export function CrossSearchPanel() {
  const { t } = useTranslation();
  const [query, setQuery] = useState("");
  const [matchContent, setMatchContent] = useState(false);
  const [result, setResult] = useState<CrossSearchOutcomeDto | null>(null);
  const [searching, setSearching] = useState(false);
  const activeWorkspaceId = useUiStore((s) => s.activeWorkspaceId);
  const openFile = useUiStore((s) => s.openFile);
  const registerCrossRef = useUiStore((s) => s.registerCrossRef);
  const focusWorkspace = useUiStore((s) => s.focusWorkspace);

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

  const handleOpenMatch = (sourceWorkspaceId: string, relativePath: string): void => {
    if (activeWorkspaceId === null) return;
    const targetWs = activeWorkspaceId;
    void focusWorkspace(targetWs);
    openFile(relativePath, targetWs);
    if (sourceWorkspaceId !== targetWs) {
      registerCrossRef(targetWs, relativePath, sourceWorkspaceId, relativePath);
    }
  };

  return (
    <div className="flex flex-col h-full bg-paper-bg">
      <div className="flex items-center gap-2 p-3 border-b border-paper-line/30">
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && search()}
          placeholder={t("crossSearch.placeholder")}
          className="flex-1 px-3 py-1.5 rounded-md border border-paper-line/40 bg-paper-bg text-sm focus:outline-none focus:border-paper-accent"
        />
        <label className="flex items-center gap-1 text-xs text-paper-muted">
          <input
            type="checkbox"
            checked={matchContent}
            onChange={(e) => setMatchContent(e.target.checked)}
          />
          {t("crossSearch.matchContent")}
        </label>
        <button
          onClick={search}
          disabled={searching}
          className="px-3 py-1.5 rounded-md bg-paper-accent text-paper-bg text-sm disabled:opacity-50"
        >
          {searching ? t("crossSearch.searching") : t("crossSearch.search")}
        </button>
      </div>
      <div className="flex-1 overflow-auto p-3">
        {result && result.skippedWorkspaceIds.length > 0 && (
          <div className="mb-3 p-2 rounded-md bg-paper-warn/10 text-xs text-paper-warn">
            {t("crossSearch.skipped", { count: result.skippedWorkspaceIds.length })}
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
                onClick={() => handleOpenMatch(group.workspaceId, match.relativePath)}
                className="ml-6 py-0.5 text-sm text-paper-muted hover:text-paper-fg cursor-pointer"
                title={activeWorkspaceId !== null ? t("crossSearch.openFromHint", { name: group.workspaceName }) : t("crossSearch.selectWorkspaceHint")}
              >
                {match.relativePath}
                <span className="ml-2 text-xs text-paper-faint">
                  {match.matchType === "fileName" ? t("crossSearch.matchTypeFileName") : t("crossSearch.matchTypeContent")}
                </span>
                {activeWorkspaceId !== null && group.workspaceId !== activeWorkspaceId && (
                  <span className="ml-2 text-xs text-paper-accent">{t("crossSearch.pullToCurrent")}</span>
                )}
              </div>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}
