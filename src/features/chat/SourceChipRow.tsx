/**
 * Single-row source-evidence chip strip pinned to the bubble's bottom edge
 * (chat_rich_bind spec 5.2.1-1/2/10). Right-aligned, total width capped at
 * 50% of the bubble, overflow collapses into a trailing `···` chip; the
 * WHOLE strip is one clickable affordance that opens the source list
 * dialog (delegated via `onOpenList`).
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { SourceEntryDto } from "../../lib/events/types";

export interface SourceChipRowProps {
  /** Collected source entries (empty arrays render nothing — callers skip). */
  sources: SourceEntryDto[];
  /** Bubble content width in px — the 50% cap baseline. */
  bubbleWidth: number;
  /** Opens the source list dialog (whole strip is the click target). */
  onOpenList: () => void;
}

/** Rough per-chip width estimate: padding + label glyphs. */
function chipWidth(label: string): number {
  return 26 + Math.min(label.length, 8) * 7;
}

/** Picks the chip labels that fit under `budget`, collapsing the rest into
 * a single `···` overflow chip (always at least one visible chip). */
export function fitChips(sources: SourceEntryDto[], budget: number): string[] {
  const labels = sources.map((s) => s.label);
  const fitting: string[] = [];
  let used = 0;
  for (const label of labels) {
    const width = chipWidth(label);
    if (used + width > budget) {
      fitting.push("···");
      return fitting;
    }
    fitting.push(label);
    used += width;
  }
  return fitting;
}

export function SourceChipRow({ sources, bubbleWidth, onOpenList }: SourceChipRowProps): ReactNode {
  const { t } = useTranslation();
  if (sources.length === 0) return null;
  const budget = Math.min(bubbleWidth * 0.5, 260);
  const chips = fitChips(sources, budget);
  if (chips.length === 0) return null;

  return (
    <button
      type="button"
      data-testid="source-chip-row"
      onClick={onOpenList}
      aria-label={t("chat.sources.openList")}
      title={t("chat.sources.openList")}
      className="mt-1 ml-auto flex max-w-[50%] shrink-0 items-center justify-end gap-1 text-xs text-ink-muted transition-colors hover:text-ink"
    >
      {chips.map((label, index) => (
        <span
          key={`${label}-${index}`}
          data-testid={label === "···" ? "source-chip-overflow" : "source-chip"}
          className="inline-flex h-5 max-w-24 items-center justify-center truncate rounded-full bg-surface-overlay px-2 leading-none"
        >
          {label}
        </span>
      ))}
    </button>
  );
}
