import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery } from "@tanstack/react-query";
import { IpcCommandError, ipc } from "../../lib/ipc/client";
import { Button } from "../../components/ui/Button";
import { Icon } from "../../components/ui/Icon/Icon";
import { useCommitAgentPref } from "./useCommitAgentPref";

interface AiCommitBarProps {
  currentMessage: string;
  onMessageChange: (message: string) => void;
}

const AI_COMMIT_ERROR_KEYS: Record<string, string> = {
  "ai_commit.no_staged_changes": "git.aiCommit.noStagedChanges",
  "ai_commit.agent_unavailable": "git.aiCommit.agentUnavailable",
  "ai_commit.generation_failed": "git.aiCommit.generationFailed",
  "ai_commit.empty_result": "git.aiCommit.emptyResult",
  "ai_commit.timeout": "git.aiCommit.timeout",
};

export function AiCommitBar({ currentMessage, onMessageChange }: AiCommitBarProps): ReactNode {
  const { t } = useTranslation();
  const pref = useCommitAgentPref();
  const [errorCode, setErrorCode] = useState<string | null>(null);
  const [showTruncated, setShowTruncated] = useState(false);

  const agentsQuery = useQuery({
    queryKey: ["ai-commit-agents"],
    queryFn: ipc.listCommitAgents,
  });

  const generateMut = useMutation({
    mutationFn: (agent: { kind: string; id: string } | null) =>
      ipc.aiCommitGenerate(agent ?? undefined),
    onSuccess: (result) => {
      setErrorCode(null);
      if (currentMessage.trim().length > 0) {
        const ok = window.confirm(t("git.aiCommit.overwriteWarning"));
        if (!ok) return;
      }
      setShowTruncated(result.truncated);
      onMessageChange(result.message);
    },
    onError: (e: unknown) => {
      if (e instanceof IpcCommandError) {
        setErrorCode(e.code);
      } else {
        setErrorCode("ai_commit.generation_failed");
      }
    },
  });

  const agents = agentsQuery.data ?? [];
  const hasAgents = agents.length > 0;
  const selectedAgent = pref.selectedAgent;
  const selectedExists =
    selectedAgent != null &&
    agents.some((a) => a.kind === selectedAgent.kind && a.id === selectedAgent.id);

  const handleGenerate = () => {
    setErrorCode(null);
    setShowTruncated(false);

    if (!pref.sensitiveAcknowledged) {
      const ok = window.confirm(t("git.aiCommit.sensitiveWarning"));
      if (!ok) return;
      pref.markSensitiveAcknowledged();
    }

    const agent = selectedExists ? selectedAgent : null;
    generateMut.mutate(agent);
  };

  const errorMessage =
    errorCode != null
      ? t(AI_COMMIT_ERROR_KEYS[errorCode] ?? "git.aiCommit.generationFailed")
      : null;

  return (
    <div className="mt-1.5 flex flex-col gap-1">
      <div className="flex items-center gap-1.5">
        <select
          aria-label={t("git.aiCommit.selectAgent")}
          className="min-w-0 flex-1 rounded border border-ink-muted/30 bg-surface px-1.5 py-1 text-xs text-ink"
          value={
            selectedExists && selectedAgent != null
              ? `${selectedAgent.kind}:${selectedAgent.id}`
              : ""
          }
          disabled={!hasAgents || generateMut.isPending}
          onChange={(ev) => {
            const val = ev.target.value;
            if (val === "") {
              pref.clearSelectedAgent();
            } else {
              const sep = val.indexOf(":");
              if (sep > 0) {
                pref.setSelectedAgent({
                  kind: val.slice(0, sep),
                  id: val.slice(sep + 1),
                });
              }
            }
          }}
        >
          <option value="">{t("git.aiCommit.defaultAgent")}</option>
          {agents.map((a) => (
            <option key={`${a.kind}:${a.id}`} value={`${a.kind}:${a.id}`}>
              {a.name}
              {a.isDefault ? ` (${t("git.aiCommit.defaultAgent")})` : ""}
            </option>
          ))}
        </select>
        <Button
          variant="outline"
          size="sm"
          disabled={!hasAgents || generateMut.isPending}
          onClick={handleGenerate}
        >
          <Icon name="commit" size={14} />
          {generateMut.isPending
            ? t("git.aiCommit.loading")
            : t("git.aiCommit.generate")}
        </Button>
      </div>

      {!hasAgents && agentsQuery.isFetched && (
        <p className="text-xs text-ink-muted">{t("git.aiCommit.noAgents")}</p>
      )}

      {errorMessage != null && (
        <p role="alert" className="text-xs text-danger">
          {errorMessage}
          <button
            type="button"
            className="ml-1.5 underline"
            onClick={() => {
              setErrorCode(null);
              handleGenerate();
            }}
          >
            {t("common.retry")}
          </button>
        </p>
      )}

      {showTruncated && (
        <p className="text-xs text-state-warn">{t("git.aiCommit.truncated")}</p>
      )}
    </div>
  );
}
