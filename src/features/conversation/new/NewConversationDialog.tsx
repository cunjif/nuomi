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
import { isRoleReady } from "../../../lib/conversation/roleReady";
import { WorkspaceSelector } from "./WorkspaceSelector";
import { hashPaletteColor } from "../avatarPalette";

export interface NewConversationDialogProps {
  kind: ConversationKind;
  onClose: () => void;
}

function initial(name: string): string {
  return (name.trim()[0] ?? "?").toUpperCase();
}

/**
 * Wizard for creating new conversations. All kinds require selecting one or
 * more Role Agents as chat participants (1 = single chat, >1 = group chat).
 */
export function NewConversationDialog({ kind, onClose }: NewConversationDialogProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const selectSession = useUiStore((s) => s.selectSession);
  const setView = useUiStore((s) => s.setView);
  const [title, setTitle] = useState("");
  const [selectedRoleIds, setSelectedRoleIds] = useState<Set<string>>(new Set());
  const [creating, setCreating] = useState(false);
  const activeWorkspaceId = useUiStore((s) => s.activeWorkspaceId);
  const [selectedWorkspaceId, setSelectedWorkspaceId] = useState<string | null>(activeWorkspaceId);
  /** Optional work-directory binding (chat_rich_bind 6.2): null = unbound. */
  const [selectedDirectoryId, setSelectedDirectoryId] = useState<string | null>(null);

  const rolesQuery = useQuery({
    queryKey: ["roles"],
    queryFn: () => ipc.listRoles(),
    staleTime: 30_000,
  });

  const workspacesQuery = useQuery({
    queryKey: ["workspaces"],
    queryFn: () => ipc.listWorkspaces(),
    staleTime: 30_000,
  });
  /** Mounted directories of the selected workspace (binding candidates). */
  const mountedDirectories = (workspacesQuery.data ?? [])
    .find((ws) => ws.id === selectedWorkspaceId)
    ?.directories?.filter((d) => d.mounted && d.available) ?? [];

  const readyRoles = (rolesQuery.data ?? []).filter(isRoleReady);

  const toggleRole = (id: string): void => {
    setSelectedRoleIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleCreate = async (): Promise<void> => {
    setCreating(true);
    try {
      const roleIds = [...selectedRoleIds];
      const firstAgent = roleIds.length > 0 ? { kind: "role" as const, id: roleIds[0]! } : null;
      const session = await ipc.createConversation({
        kind,
        title: title.trim() || null,
        agent: firstAgent,
        teamId: null,
        workspaceId: selectedWorkspaceId ?? "__migrated__",
        workDirectoryId: selectedDirectoryId,
      });
      for (let i = 1; i < roleIds.length; i++) {
        await ipc.addConversationAgent(session.id, { kind: "role", id: roleIds[i]! });
      }
      void qc.invalidateQueries({ queryKey: ["conversations"] });
      selectSession(session.id);
      onClose();
    } catch (e) {
      const msg = describeError(e);
      if (msg.includes("workspace_not_found")) {
        toast.error(t("conversation.workspaceNotFoundInDialog"));
        void qc.invalidateQueries({ queryKey: ["workspaces"] });
      } else {
        toast.error(msg);
      }
    } finally {
      setCreating(false);
    }
  };

  const canCreate = selectedRoleIds.size > 0 && !creating && selectedWorkspaceId !== null;

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
            disabled={!canCreate}
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
        <WorkspaceSelector
          selectedWorkspaceId={selectedWorkspaceId}
          onSelect={(id) => {
            setSelectedWorkspaceId(id);
            // Directory candidates are workspace-scoped: reset the binding
            // whenever the workspace changes (a stale id would be rejected
            // by the backend's same-workspace validation anyway).
            setSelectedDirectoryId(null);
          }}
        />
        <div data-testid="directory-selector">
          <label className="mb-1 block text-xs text-ink-muted">{t("conversation.newDialogDirectory")}</label>
          <div className="flex flex-col gap-1 rounded border border-ink-muted/40 bg-surface-raised p-2">
            <button
              type="button"
              onClick={() => setSelectedDirectoryId(null)}
              className={`flex items-center gap-2 rounded px-2 py-1.5 text-sm text-ink transition-colors ${
                selectedDirectoryId === null ? "bg-ink-accent/15" : "hover:bg-surface-overlay"
              }`}
            >
              <span className="flex-1 text-left">{t("conversation.newDialogDirectoryNone")}</span>
              <span
                className={`flex h-4 w-4 shrink-0 items-center justify-center rounded border ${
                  selectedDirectoryId === null ? "border-ink-accent bg-ink-accent text-surface" : "border-ink-muted/50"
                }`}
              >
                {selectedDirectoryId === null ? "✓" : ""}
              </span>
            </button>
            {mountedDirectories.map((dir) => {
              const checked = selectedDirectoryId === dir.id;
              return (
                <button
                  key={dir.id}
                  type="button"
                  title={dir.absPath}
                  onClick={() => setSelectedDirectoryId(dir.id)}
                  className={`flex items-center gap-2 rounded px-2 py-1.5 text-sm text-ink transition-colors ${
                    checked ? "bg-ink-accent/15" : "hover:bg-surface-overlay"
                  }`}
                >
                  <span className="flex-1 truncate text-left">{dir.displayName}</span>
                  <span
                    className={`flex h-4 w-4 shrink-0 items-center justify-center rounded border ${
                      checked ? "border-ink-accent bg-ink-accent text-surface" : "border-ink-muted/50"
                    }`}
                  >
                    {checked ? "✓" : ""}
                  </span>
                </button>
              );
            })}
          </div>
          <p className="mt-1 text-xs text-ink-muted">{t("conversation.newDialogDirectoryHint")}</p>
        </div>
        <div>
          <label className="mb-1 block text-xs text-ink-muted">{t("conversation.newDialogSelectAgents")}</label>
          {readyRoles.length === 0 ? (
            <div className="flex flex-col items-center gap-2 rounded border border-ink-muted/40 bg-surface-raised p-4 text-center">
              <p className="text-sm text-ink-muted">{t("conversation.newDialogNoRoleAgent")}</p>
              <button
                type="button"
                onClick={() => { setView("settings"); onClose(); }}
                className="rounded border border-ink-accent px-2 py-0.5 text-xs text-ink-accent hover:bg-ink-accent hover:text-surface"
              >
                {t("conversation.newDialogGoToSettings")}
              </button>
            </div>
          ) : (
            <div className="flex flex-col gap-1 max-h-48 overflow-y-auto rounded border border-ink-muted/40 bg-surface-raised p-2">
              {readyRoles.map((role) => {
                const checked = selectedRoleIds.has(role.id);
                const palette = hashPaletteColor(role.id);
                return (
                  <button
                    key={role.id}
                    type="button"
                    onClick={() => toggleRole(role.id)}
                    className={`flex items-center gap-2 rounded px-2 py-1.5 text-sm text-ink transition-colors ${checked ? "bg-ink-accent/15" : "hover:bg-surface-overlay"}`}
                  >
                    <span
                      className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-xs font-medium"
                      style={{ backgroundColor: palette.bg, color: palette.fg }}
                    >
                      {initial(role.name)}
                    </span>
                    <span className="flex-1 text-left">{role.name}</span>
                    <span
                      className={`flex h-4 w-4 shrink-0 items-center justify-center rounded border ${checked ? "border-ink-accent bg-ink-accent text-surface" : "border-ink-muted/50"}`}
                    >
                      {checked ? "✓" : ""}
                    </span>
                  </button>
                );
              })}
            </div>
          )}
          {readyRoles.length > 0 && (
            <p className="mt-1 text-xs text-ink-muted">
              {selectedRoleIds.size === 0
                ? t("conversation.newDialogRoleAgentHint")
                : selectedRoleIds.size === 1
                  ? t("conversation.newDialogSingleChat")
                  : t("conversation.newDialogGroupChat")}
            </p>
          )}
        </div>
      </div>
    </Dialog>
  );
}
