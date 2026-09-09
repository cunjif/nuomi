import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { TraceEntry } from "./traceModel";

interface TimelineRowProps {
  entry: TraceEntry;
  speakerFallback: string;
}

/** One timeline row: speech bubble, folded tool card or whiteboard note. */
export function TimelineRow({ entry, speakerFallback }: TimelineRowProps): ReactNode {
  const { t } = useTranslation();
  if (entry.kind === "speech") {
    return (
      <div className="animate-draw-in px-3 py-1">
        <span className="mb-0.5 inline-block rounded-full border border-dashed border-ink-accent/60 px-1.5 font-scribble text-xs leading-tight text-ink-accent">
          {entry.speaker || speakerFallback}
        </span>
        <p className="whitespace-pre-wrap break-words text-sm text-ink">{entry.content}</p>
      </div>
    );
  }
  if (entry.kind === "tool") {
    return (
      <details className="sketch-card animate-draw-in mx-3 my-1 bg-surface-raised text-sm">
        <summary className="cursor-pointer px-3 py-1.5 text-xs text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent">
          {t("chat.toolCall")}
          <span className="ml-2 font-mono text-ink-accent">{entry.tool}</span>
        </summary>
        <pre className="overflow-x-auto px-3 pb-2 font-mono text-xs text-ink-muted">{entry.argsJson}</pre>
      </details>
    );
  }
  if (entry.kind === "tool_result") {
    return (
      <details className="sketch-card animate-draw-in mx-3 my-1 bg-surface-raised text-sm">
        <summary className="cursor-pointer px-3 py-1.5 text-xs text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent">
          {t("chat.toolResult")}
        </summary>
        <pre className="overflow-x-auto px-3 pb-2 font-mono text-xs text-ink-muted">{entry.content}</pre>
      </details>
    );
  }
  return (
    <div className="sketch-card animate-draw-in mx-3 my-1 border-state-warn/60 bg-surface-raised p-2">
      <p className="text-xs font-semibold text-state-warn">{t("trace.whiteboardHeading")}</p>
      <p className="whitespace-pre-wrap break-words text-sm text-ink">{entry.body}</p>
    </div>
  );
}
