import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Dialog } from "../../../components/ui/Dialog";
import { ipc } from "../../../lib/ipc/client";
import { toast } from "../../../lib/store/toastStore";
import { useUiStore } from "../../../lib/store/uiStore";
import { describeError } from "../../../i18n";
import type { ConversationKind } from "../../../lib/conversation/kinds";

export interface NewConversationDialogProps {
  kind: ConversationKind;
  onClose: () => void;
}

/**
 * Type-specific wizard for creating new conversations. Fields adapt to the
 * selected kind: chat (title + agent), group (team), background (task desc),
 * scheduled (cron + target).
 */
export function NewConversationDialog({ kind, onClose }: NewConversationDialogProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const selectSession = useUiStore((s) => s.selectSession);
  const [title, setTitle] = useState("");
  const [teamId, setTeamId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const teamsQuery = useQuery({
    queryKey: ["teams"],
    queryFn: () => ipc.listTeams(),
    enabled: kind === "group",
    staleTime: 30_000,
  });

  const handleCreate = async (): Promise<void> => {
    setCreating(true);
    try {
      const session = await ipc.createConversation({
        kind,
        title: title.trim() || null,
        agent: null,
        teamId: kind === "group" ? teamId : null,
      });
      void qc.invalidateQueries({ queryKey: ["conversations"] });
      selectSession(session.id);
      onClose();
    } catch (e) {
      toast.error(describeError(e));
    } finally {
      setCreating(false);
    }
  };

  return (
    <Dialog
      open
      title={t("conversation.newDialogTitle")}
      onClose={onClose}
      footer={
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded border border-ink-muted/40 px-3 py-1 text-sm text-ink-muted hover:bg-surface-overlay"
          >
            {t("conversation.newDialogCancel")}
          </button>
          <button
            type="button"
            onClick={() => void handleCreate()}
            disabled={creating}
            className="pixel-fill-accent rounded px-3 py-1 text-sm text-surface disabled:opacity-50"
          >
            {t("conversation.newDialogCreate")}
          </button>
        </div>
      }
    >
      <div className="flex flex-col gap-3 py-2">
        <div>
          <label className="mb-1 block text-xs text-ink-muted">{t("conversation.newDialogKind")}</label>
          <span className="rounded bg-ink-muted/20 px-2 py-0.5 text-sm text-ink">{kind}</span>
        </div>
        <div>
          <label className="mb-1 block text-xs text-ink-muted">{t("conversation.newDialogTitleLabel")}</label>
          <input
            type="text"
            value={title}
            onChange={(e) => setTitle(e.target.value)}
            className="w-full rounded border border-ink-muted/40 bg-surface-raised px-2 py-1 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
          />
        </div>
        {kind === "group" && (
          <div>
            <label className="mb-1 block text-xs text-ink-muted">{t("conversation.newDialogTeam")}</label>
            <select
              value={teamId ?? ""}
              onChange={(e) => setTeamId(e.target.value || null)}
              className="w-full rounded border border-ink-muted/40 bg-surface-raised px-2 py-1 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent"
            >
              <option value="">—</option>
              {(teamsQuery.data ?? []).map((team) => (
                <option key={team.id} value={team.id}>{team.name}</option>
              ))}
            </select>
          </div>
        )}
      </div>
    </Dialog>
  );
}
