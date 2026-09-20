import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { describeError } from "../../i18n";
import { ipc } from "../../lib/ipc/client";
import { toast } from "../../lib/store/toastStore";
import { ToolTagInput } from "./ToolTagInput";

type RefineStrategy = "prompt_note" | "memory" | "skill" | "sub_agent_spec";
type SkillFormat = "skill_md";
type RetrievalStrategy = "keyword" | "semantic" | "hybrid";

interface EvolutionSettingsState {
  onlineAuthorized: boolean;
  onlineAllowlist: string[];
  triggerFailures: number;
  minEditStrategy: RefineStrategy;
  evidenceThreshold: number;
  rollbackEnabled: boolean;
  skillCreationEnabled: boolean;
  skillFormat: SkillFormat;
  retentionDays: number;
  retrieval: RetrievalStrategy;
}

const DEFAULTS: EvolutionSettingsState = {
  onlineAuthorized: false,
  onlineAllowlist: [],
  triggerFailures: 3,
  minEditStrategy: "prompt_note",
  evidenceThreshold: 0.8,
  rollbackEnabled: true,
  skillCreationEnabled: true,
  skillFormat: "skill_md",
  retentionDays: 90,
  retrieval: "keyword",
};

/** Four-dimension evolution settings panel (Continual Harness H=(ρ,G,K,M)). */
export function EvolutionSettingsPanel(): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const [state, setState] = useState<EvolutionSettingsState>(DEFAULTS);

  const authQuery = useQuery({
    queryKey: ["onlineAuthorized"],
    queryFn: ipc.getOnlineAuthorized,
  });

  const authMut = useMutation({
    mutationFn: (authorized: boolean) => ipc.setOnlineAuthorized(authorized),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["onlineAuthorized"] });
      toast.success(t("settings.evolution.saved"));
    },
    onError: (e) => toast.error(`${t("settings.evolution.saveFailed")}: ${describeError(e)}`),
  });

  const update = <K extends keyof EvolutionSettingsState>(
    key: K,
    value: EvolutionSettingsState[K],
  ) => setState((prev) => ({ ...prev, [key]: value }));

  return (
    <section aria-label={t("settings.evolution.heading")} className="flex flex-col gap-3">
      <h3 className="text-sm font-semibold text-ink">{t("settings.evolution.heading")}</h3>

      {/* Dimension 1: Online learning allowlist */}
      <div className="sketch-card bg-surface-raised p-3">
        <h4 className="mb-2 text-xs font-semibold text-ink">
          {t("settings.evolution.online.title")}
        </h4>
        <p className="mb-2 text-[10px] text-ink-muted">
          {t("settings.evolution.online.description")}
        </p>
        <label className="mb-2 flex items-center gap-2 text-xs text-ink-muted">
          <input
            type="checkbox"
            checked={authQuery.data ?? state.onlineAuthorized}
            onChange={(e) => {
              update("onlineAuthorized", e.target.checked);
              authMut.mutate(e.target.checked);
            }}
            className="accent-[var(--nuomi-accent)]"
          />
          {t("settings.evolution.online.authorized")}
        </label>
        <label className="flex flex-col gap-1 text-xs text-ink-muted">
          {t("settings.evolution.online.allowlist")}
          <ToolTagInput
            value={state.onlineAllowlist}
            onChange={(next) => update("onlineAllowlist", next)}
            candidates={[]}
            placeholder="example.com"
            emptyHintLabel={t("settings.evolution.online.allowlistEmpty")}
          />
        </label>
      </div>

      {/* Dimension 2: Reflection / refine parameters */}
      <div className="sketch-card bg-surface-raised p-3">
        <h4 className="mb-2 text-xs font-semibold text-ink">
          {t("settings.evolution.refine.title")}
        </h4>
        <p className="mb-2 text-[10px] text-ink-muted">
          {t("settings.evolution.refine.description")}
        </p>
        <div className="grid grid-cols-2 gap-2">
          <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
            {t("settings.evolution.refine.triggerFailures")}
            <input
              type="number"
              min={1}
              max={10}
              value={state.triggerFailures}
              onChange={(e) => update("triggerFailures", parseInt(e.target.value, 10) || 1)}
              className="rounded border border-ink-muted/40 bg-surface px-1.5 py-0.5 text-xs"
            />
          </label>
          <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
            {t("settings.evolution.refine.minEditStrategy")}
            <select
              value={state.minEditStrategy}
              onChange={(e) => update("minEditStrategy", e.target.value as RefineStrategy)}
              className="rounded border border-ink-muted/40 bg-surface px-1.5 py-0.5 text-xs"
            >
              <option value="prompt_note">prompt_note</option>
              <option value="memory">memory</option>
              <option value="skill">skill</option>
              <option value="sub_agent_spec">sub_agent_spec</option>
            </select>
          </label>
          <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
            {t("settings.evolution.refine.evidenceThreshold")}
            <span className="flex items-center gap-2">
              <input
                type="range"
                min={0.5}
                max={1}
                step={0.05}
                value={state.evidenceThreshold}
                onChange={(e) => update("evidenceThreshold", parseFloat(e.target.value))}
                className="flex-1 accent-[var(--nuomi-accent)]"
              />
              <span className="w-8 tabular-nums">{state.evidenceThreshold.toFixed(2)}</span>
            </span>
          </label>
          <label className="flex items-center gap-2 text-xs text-ink-muted">
            <input
              type="checkbox"
              checked={state.rollbackEnabled}
              onChange={(e) => update("rollbackEnabled", e.target.checked)}
              className="accent-[var(--nuomi-accent)]"
            />
            {t("settings.evolution.refine.rollbackEnabled")}
          </label>
        </div>
      </div>

      {/* Dimension 3: Auto skill creation */}
      <div className="sketch-card bg-surface-raised p-3">
        <h4 className="mb-2 text-xs font-semibold text-ink">
          {t("settings.evolution.skill.title")}
        </h4>
        <p className="mb-2 text-[10px] text-ink-muted">
          {t("settings.evolution.skill.description")}
        </p>
        <label className="mb-2 flex items-center gap-2 text-xs text-ink-muted">
          <input
            type="checkbox"
            checked={state.skillCreationEnabled}
            onChange={(e) => update("skillCreationEnabled", e.target.checked)}
            className="accent-[var(--nuomi-accent)]"
          />
          {t("settings.evolution.skill.enabled")}
        </label>
        <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
          {t("settings.evolution.skill.format")}
          <select
            value={state.skillFormat}
            onChange={(e) => update("skillFormat", e.target.value as SkillFormat)}
            className="rounded border border-ink-muted/40 bg-surface px-1.5 py-0.5 text-xs"
          >
            <option value="skill_md">SKILL.md</option>
          </select>
        </label>
      </div>

      {/* Dimension 4: Persistent memory policy */}
      <div className="sketch-card bg-surface-raised p-3">
        <h4 className="mb-2 text-xs font-semibold text-ink">
          {t("settings.evolution.memory.title")}
        </h4>
        <p className="mb-2 text-[10px] text-ink-muted">
          {t("settings.evolution.memory.description")}
        </p>
        <div className="grid grid-cols-2 gap-2">
          <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
            {t("settings.evolution.memory.retentionDays")}
            <input
              type="number"
              min={1}
              max={365}
              value={state.retentionDays}
              onChange={(e) => update("retentionDays", parseInt(e.target.value, 10) || 1)}
              className="rounded border border-ink-muted/40 bg-surface px-1.5 py-0.5 text-xs"
            />
          </label>
          <label className="flex flex-col gap-0.5 text-xs text-ink-muted">
            {t("settings.evolution.memory.retrieval")}
            <select
              value={state.retrieval}
              onChange={(e) => update("retrieval", e.target.value as RetrievalStrategy)}
              className="rounded border border-ink-muted/40 bg-surface px-1.5 py-0.5 text-xs"
            >
              <option value="keyword">{t("settings.evolution.memory.retrievalKeyword")}</option>
              <option value="semantic">{t("settings.evolution.memory.retrievalSemantic")}</option>
              <option value="hybrid">{t("settings.evolution.memory.retrievalHybrid")}</option>
            </select>
          </label>
        </div>
      </div>
    </section>
  );
}
