import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import type { ScheduleInput } from "../../lib/ipc/bindings.gen";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";
import { IpcCommandError } from "../../lib/ipc/client";
import { isRoleReady } from "../../lib/conversation/roleReady";

type TargetKind = "task" | "chat" | "group";
type TeamSource = "auto" | "pick";

interface ScheduleFormProps {
  onDone: () => void;
}

/** Create-schedule form exposing the full schedule surface: target kind,
 * chat agent, group team selection vs spontaneous formation, auto dispatch. */
export function ScheduleForm({ onDone }: ScheduleFormProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const activeWorkspaceId = useUiStore((s) => s.activeWorkspaceId);
  const [name, setName] = useState("");
  const [cronExpr, setCronExpr] = useState("");
  const [taskTitle, setTaskTitle] = useState("");
  const [taskDescription, setTaskDescription] = useState("");
  const [targetKind, setTargetKind] = useState<TargetKind>("task");
  const [agentId, setAgentId] = useState("");
  const [teamSource, setTeamSource] = useState<TeamSource>("auto");
  const [teamId, setTeamId] = useState("");
  const [autoDispatch, setAutoDispatch] = useState(true);

  const rolesQuery = useQuery({
    queryKey: ["roles"],
    queryFn: () => ipc.listRoles(),
    staleTime: 30_000,
    enabled: targetKind === "chat",
  });
  const teamsQuery = useQuery({
    queryKey: ["teams"],
    queryFn: () => ipc.listTeams(),
    staleTime: 30_000,
    enabled: targetKind === "group",
  });
  const readyRoles = (rolesQuery.data ?? []).filter(isRoleReady);

  const createMut = useMutation({
    mutationFn: (input: ScheduleInput) => ipc.upsertSchedule(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["schedules"] });
      setName("");
      setCronExpr("");
      setTaskTitle("");
      setTaskDescription("");
      setAgentId("");
      setTeamId("");
      setTargetKind("task");
      setTeamSource("auto");
      setAutoDispatch(true);
      onDone();
    },
    onError: (e) => {
      if (e instanceof IpcCommandError && e.code === "scheduler.no_focused_workspace") {
        toast.error(t("scheduler.noFocusedWorkspace"));
        return;
      }
      if (e instanceof IpcCommandError && e.code === "scheduler.duplicate_name") {
        toast.error(t("scheduler.duplicateName"));
        return;
      }
      toast.error(`${t("scheduler.createFailed")}: ${describeError(e)}`);
    },
  });

  const field =
    "w-48 rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent";

  const kindLabel: Record<TargetKind, string> = {
    task: t("scheduler.targetTask"),
    chat: t("scheduler.targetChat"),
    group: t("scheduler.targetGroup"),
  };

  const submit = (): void => {
    if (targetKind === "chat" && !agentId) {
      toast.error(t("scheduler.agentRequired"));
      return;
    }
    if (targetKind === "group" && teamSource === "pick" && !teamId) {
      toast.error(t("scheduler.teamRequired"));
      return;
    }
    createMut.mutate({
      name: name.trim(),
      cronExpr: cronExpr.trim(),
      targetKind,
      agent: targetKind === "chat" && agentId ? { kind: "role", id: agentId } : null,
      teamId: targetKind === "group" && teamSource === "pick" && teamId ? teamId : null,
      sessionMode: "per_trigger",
      sessionId: null,
      autoDispatch: targetKind === "task" ? true : autoDispatch,
      taskTitle: taskTitle.trim(),
      taskDescription: taskDescription.trim(),
    });
  };

  return (
    <form
      aria-label={t("scheduler.heading")}
      className="mb-2 flex flex-wrap items-end gap-2 rounded border border-ink-muted/40 bg-surface-raised p-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (!activeWorkspaceId) {
          toast.error(t("scheduler.noFocusedWorkspace"));
          return;
        }
        if (name.trim() && cronExpr.trim() && taskTitle.trim()) submit();
      }}
    >
      <div className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("scheduler.targetKind")}
        <div className="flex items-center gap-1" role="group" aria-label={t("scheduler.targetKind")}>
          {(["task", "chat", "group"] as const).map((kind) => (
            <button
              key={kind}
              type="button"
              aria-pressed={targetKind === kind}
              onClick={() => setTargetKind(kind)}
              className={`rounded px-2 py-1 text-xs transition-colors focus-visible:ring-2 focus-visible:ring-ink-accent ${
                targetKind === kind
                  ? "bg-ink-accent text-surface"
                  : "bg-surface-raised text-ink-muted hover:text-ink"
              }`}
            >
              {kindLabel[kind]}
            </button>
          ))}
        </div>
      </div>
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("scheduler.name")}
        <input value={name} onChange={(e) => setName(e.target.value)} required className={field} />
      </label>
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("scheduler.cronExpr")}
        <input value={cronExpr} onChange={(e) => setCronExpr(e.target.value)} required placeholder="0 9 * * *" className={`${field} font-mono`} />
      </label>
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("scheduler.taskTitle")}
        <input value={taskTitle} onChange={(e) => setTaskTitle(e.target.value)} required className={field} />
      </label>
      <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
        {t("scheduler.taskDescription")}
        <input value={taskDescription} onChange={(e) => setTaskDescription(e.target.value)} className="w-64 rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent" />
      </label>
      {targetKind === "chat" && (
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("scheduler.agentLabel")}
          <select value={agentId} onChange={(e) => setAgentId(e.target.value)} required className={field}>
            <option value="">{t("scheduler.agentPlaceholder")}</option>
            {readyRoles.map((role) => (
              <option key={role.id} value={role.id}>
                {role.name}
              </option>
            ))}
          </select>
        </label>
      )}
      {targetKind === "group" && (
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("scheduler.teamSource")}
          <select value={teamSource} onChange={(e) => setTeamSource(e.target.value as TeamSource)} className={field}>
            <option value="auto">{t("scheduler.teamAuto")}</option>
            <option value="pick">{t("scheduler.teamPick")}</option>
          </select>
        </label>
      )}
      {targetKind === "group" && teamSource === "pick" && (
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("scheduler.teamPick")}
          <select value={teamId} onChange={(e) => setTeamId(e.target.value)} required className={field}>
            <option value="">{t("scheduler.teamPlaceholder")}</option>
            {(teamsQuery.data ?? []).map((team) => (
              <option key={team.id} value={team.id}>
                {team.name}
              </option>
            ))}
          </select>
        </label>
      )}
      {targetKind !== "task" && (
        <label className="flex items-center gap-1 pb-1 text-xs text-ink-muted">
          <input
            type="checkbox"
            checked={autoDispatch}
            onChange={(e) => setAutoDispatch(e.target.checked)}
            className="accent-[var(--nuomi-accent)]"
          />
          {t("scheduler.autoDispatch")}
        </label>
      )}
      <button
        type="submit"
        disabled={createMut.isPending}
        className="pixel-fill-accent px-3 py-1.5 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      >
        {t("common.create")}
      </button>
      <button
        type="button"
        onClick={onDone}
        className="rounded border border-ink-muted px-3 py-1.5 text-sm text-ink-muted hover:bg-surface-overlay focus-visible:ring-2 focus-visible:ring-ink-accent"
      >
        {t("common.cancel")}
      </button>
    </form>
  );
}
