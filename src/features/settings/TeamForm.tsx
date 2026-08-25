import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { JsonValue, TeamInput, TeamTopologyDto } from "../../lib/ipc/bindings.gen";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { TeamMemberPicker } from "./TeamMemberPicker";

const TOPOLOGY_LABEL_KEYS: Record<TeamTopologyDto, string> = {
  pipeline: "settings.teams.topologyPipeline",
  router: "settings.teams.topologyRouter",
  group_chat: "settings.teams.topologyGroupChat",
};

/** Reads `details.missing` off a team.member_missing IpcCommandError. */
export function extractMissingMembers(error: unknown): string[] | null {
  if (!(error instanceof Error) || !("code" in error)) return null;
  const code = (error as { code?: unknown }).code;
  const details = (error as { details?: unknown }).details;
  if (code !== "team.member_missing") return null;
  if (details === null || typeof details !== "object" || Array.isArray(details)) return null;
  const missing = (details as Record<string, unknown>)["missing"];
  if (!Array.isArray(missing)) return null;
  return missing.map((m) => String(m));
}

/** Add form for teams: topology + ordered member roles (`name` idempotency key). */
export function TeamForm(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [name, setName] = useState("");
  const [topology, setTopology] = useState<TeamTopologyDto>("pipeline");
  /** Ordered member role ids — the order becomes memberRoleIds. */
  const [memberIds, setMemberIds] = useState<string[]>([]);
  const [maxRounds, setMaxRounds] = useState(6);

  const rolesQuery = useQuery({ queryKey: ["roles"], queryFn: ipc.listRoles });
  const roles = rolesQuery.data ?? [];
  const roleName = new Map(roles.map((r) => [r.id, r.name]));

  const saveMut = useMutation({
    mutationFn: (input: TeamInput) => ipc.upsertTeam(input),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["teams"] });
      toast.success(t("settings.teams.saved"));
      setName("");
      setTopology("pipeline");
      setMemberIds([]);
      setMaxRounds(6);
    },
    onError: (e) => {
      // Inputs stay as-is so the user can fix and resubmit.
      const missing = extractMissingMembers(e);
      const detail =
        missing !== null
          ? t("settings.teams.missingMembers", {
              names: missing.map((id) => roleName.get(id) ?? id).join(", "),
            })
          : describeError(e);
      toast.error(`${t("settings.teams.saveFailed")}: ${detail}`);
    },
  });

  const field =
    "rounded border border-ink-muted/40 bg-surface px-2 py-1 text-sm text-ink focus-visible:ring-2 focus-visible:ring-ink-accent";

  const toggleMember = (roleId: string): void => {
    setMemberIds((prev) =>
      prev.includes(roleId) ? prev.filter((id) => id !== roleId) : [...prev, roleId],
    );
  };

  const moveMember = (roleId: string, delta: -1 | 1): void => {
    setMemberIds((prev) => {
      const i = prev.indexOf(roleId);
      const j = i + delta;
      if (i < 0 || j < 0 || j >= prev.length) return prev;
      const next = [...prev];
      const moved = next[j];
      const displaced = next[i];
      if (moved === undefined || displaced === undefined) return prev;
      next[j] = displaced;
      next[i] = moved;
      return next;
    });
  };

  return (
    <form
      aria-label={t("settings.teams.heading")}
      className="mb-3 rounded border border-ink-muted/40 bg-surface-raised p-3"
      onSubmit={(e) => {
        e.preventDefault();
        if (!name.trim() || memberIds.length === 0 || saveMut.isPending) return;
        const config: JsonValue =
          topology === "group_chat" ? { max_rounds: maxRounds } : {};
        saveMut.mutate({
          name: name.trim(),
          topology,
          memberRoleIds: [...memberIds],
          config,
        });
      }}
    >
      <p className="mb-2 text-xs text-ink-muted">{t("settings.teams.idempotentHint")}</p>
      <div className="flex flex-wrap items-end gap-2">
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.teams.name")}
          <input value={name} onChange={(e) => setName(e.target.value)} required className={`${field} w-40`} />
        </label>
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.teams.topology")}
          <select
            value={topology}
            onChange={(e) => {
              const value = e.target.value;
              if (value === "pipeline" || value === "router" || value === "group_chat") {
                setTopology(value);
              }
            }}
            className={field}
          >
            {(Object.keys(TOPOLOGY_LABEL_KEYS) as TeamTopologyDto[]).map((top) => (
              <option key={top} value={top}>
                {t(TOPOLOGY_LABEL_KEYS[top])}
              </option>
            ))}
          </select>
        </label>
        {topology === "group_chat" && (
          <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
            {t("settings.teams.maxRounds")}
            <input
              type="number"
              min={1}
              step={1}
              value={maxRounds}
              onChange={(e) => {
                const parsed = Number.parseInt(e.target.value, 10);
                setMaxRounds(Number.isNaN(parsed) ? 1 : parsed);
              }}
              className={`${field} w-20`}
            />
          </label>
        )}
      </div>
      <div className="mt-2">
        <TeamMemberPicker
          roles={roles}
          memberIds={memberIds}
          onToggle={toggleMember}
          onMove={moveMember}
        />
      </div>
      <button
        type="submit"
        disabled={saveMut.isPending}
        className="mt-2 rounded bg-ink-accent px-3 py-1.5 text-sm text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
      >
        {saveMut.isPending ? t("settings.teams.saving") : t("settings.teams.save")}
      </button>
    </form>
  );
}
