import type { ReactNode } from "react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { CapabilityDto, ModelEntryDto } from "../../lib/ipc/bindings.gen";
import { fieldClass as field } from "../../components/ui/Field";
import { recommendHyperparams } from "./recommendHyperparams";

const CAP_KEYS: ReadonlyArray<{ key: CapabilityDto; short: string; cls: string }> = [
  { key: "reasoning", short: "Re", cls: "border-cap-re text-cap-re" },
  { key: "image", short: "I", cls: "border-cap-i text-cap-i" },
  { key: "voice", short: "Vo", cls: "border-cap-vo text-cap-vo" },
  { key: "video", short: "Vi", cls: "border-cap-vi text-cap-vi" },
];

interface ModelHyperParamsRowProps {
  model: ModelEntryDto;
  onChange: (next: ModelEntryDto) => void;
  onRemove: () => void;
}

/** Per-model row: inline capability toggles + collapsible hyperparams + auto-recommend. */
export function ModelHyperParamsRow({ model, onChange, onRemove }: ModelHyperParamsRowProps): ReactNode {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);

  const toggleCap = (cap: CapabilityDto) => {
    const caps = model.capabilities.includes(cap)
      ? model.capabilities.filter((c) => c !== cap)
      : [...model.capabilities, cap];
    onChange({ ...model, capabilities: caps });
  };

  const applyRecommend = () => {
    const rec = recommendHyperparams(model.capabilities);
    onChange({
      ...model,
      temperature: rec.temperature,
      topP: rec.topP,
      maxTokens: rec.maxTokens,
    });
  };

  return (
    <div className="rounded border border-ink-muted/30 bg-surface p-2">
      <div className="flex flex-wrap items-center gap-2">
        <input
          value={model.id}
          onChange={(e) => onChange({ ...model, id: e.target.value })}
          className={`${field} bg-surface-raised text-xs font-mono w-40`}
          aria-label={t("provider.modelId")}
        />
        {CAP_KEYS.map((cap) => {
          const active = model.capabilities.includes(cap.key);
          return (
            <button
              key={cap.key}
              type="button"
              onClick={() => toggleCap(cap.key)}
              className={`rounded border px-1.5 py-0.5 text-[10px] leading-none ${
                active ? cap.cls : "border-ink-muted/40 text-ink-muted"
              }`}
            >
              {cap.short}
            </button>
          );
        })}
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className="ml-auto rounded border border-ink-muted px-1.5 py-0.5 text-[10px] text-ink-muted hover:bg-surface-overlay"
        >
          {expanded ? t("provider.modelCollapse") : t("provider.modelExpand")}
        </button>
        <button
          type="button"
          onClick={onRemove}
          className="rounded border border-state-danger px-1.5 py-0.5 text-[10px] text-state-danger hover:bg-surface-overlay"
          aria-label={t("common.delete")}
        >
          ×
        </button>
      </div>
      {expanded && (
        <div className="mt-2 flex flex-wrap items-end gap-2 border-t border-ink-muted/20 pt-2">
          <label className="flex flex-col gap-0.5 text-[10px] text-ink-muted">
            {t("provider.temperature")}
            <input
              type="range"
              min="0"
              max="2"
              step="0.1"
              value={model.temperature ?? 0.7}
              onChange={(e) => onChange({ ...model, temperature: parseFloat(e.target.value) })}
              className="w-24"
            />
            <span className="tabular-nums">{(model.temperature ?? 0.7).toFixed(1)}</span>
          </label>
          <label className="flex flex-col gap-0.5 text-[10px] text-ink-muted">
            {t("provider.topP")}
            <input
              type="range"
              min="0"
              max="1"
              step="0.05"
              value={model.topP ?? 1}
              onChange={(e) => onChange({ ...model, topP: parseFloat(e.target.value) })}
              className="w-24"
            />
            <span className="tabular-nums">{(model.topP ?? 1).toFixed(2)}</span>
          </label>
          <label className="flex flex-col gap-0.5 text-[10px] text-ink-muted">
            {t("provider.maxTokens")}
            <input
              type="number"
              value={model.maxTokens?.toString() ?? ""}
              onChange={(e) =>
                onChange({ ...model, maxTokens: e.target.value.length > 0 ? parseInt(e.target.value, 10) : null })
              }
              className={`${field} bg-surface-raised text-xs w-24`}
            />
          </label>
          <button
            type="button"
            onClick={applyRecommend}
            className="rounded border border-ink-accent px-2 py-0.5 text-[10px] text-ink-accent hover:bg-ink-accent/10"
          >
            {t("provider.recommendHyperparams")}
          </button>
        </div>
      )}
    </div>
  );
}
