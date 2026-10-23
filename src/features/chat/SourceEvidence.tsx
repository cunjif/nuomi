/**
 * Source evidence bundle for one assistant bubble: the chip row (pinned to
 * the bubble's bottom edge) + the list dialog it opens. Jump routing is
 * delegated to `navigateSource` (sourceNavigation service, task 4.4 — a
 * stub until that lands: jumps are logged and dropped).
 */
import { useState, type ReactNode } from "react";
import { SourceChipRow } from "./SourceChipRow";
import { SourceListDialog } from "./SourceListDialog";
import { navigateSource } from "./sourceNavigation";
import type { SourceEntryDto } from "../../lib/events/types";

export interface SourceEvidenceProps {
  sources: SourceEntryDto[];
}

export function SourceEvidence({ sources }: SourceEvidenceProps): ReactNode {
  const [open, setOpen] = useState(false);
  const [bubbleWidth, setBubbleWidth] = useState(360);

  if (sources.length === 0) return null;

  return (
    <div
      ref={(node) => {
        const host = node?.parentElement ?? null;
        if (host !== null && host.clientWidth > 0) {
          setBubbleWidth(host.clientWidth);
        }
      }}
    >
      <SourceChipRow sources={sources} bubbleWidth={bubbleWidth} onOpenList={() => setOpen(true)} />
      <SourceListDialog
        open={open}
        sources={sources}
        onClose={() => setOpen(false)}
        onNavigate={(source) => {
          void navigateSource(source);
        }}
      />
    </div>
  );
}
