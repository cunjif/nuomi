/**
 * Source-evidence list dialog (chat_rich_bind spec 5.2.1-3): one row per
 * collected source — full file paths / URLs plus an open action. Jump
 * routing (workbench focus vs. temp tab vs. system browser) is delegated
 * to the caller's `onNavigate` (the sourceNavigation service, task 4.4);
 * `cli_agent` entries and empty targets render disabled (nothing to open).
 */
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Dialog } from "../../components/ui/Dialog";
import type { SourceEntryDto } from "../../lib/events/types";

export interface SourceListDialogProps {
  open: boolean;
  sources: SourceEntryDto[];
  onClose: () => void;
  /** Invoked with the entry to open; receives every non-cli_agent entry. */
  onNavigate: (source: SourceEntryDto) => void;
}

function kindLabelKey(kind: SourceEntryDto["kind"]): string {
  switch (kind) {
    case "web_url":
      return "chat.sources.link";
    case "cli_agent":
      return "chat.sources.agent";
    default:
      return "chat.sources.file";
  }
}

export function SourceListDialog({ open, sources, onClose, onNavigate }: SourceListDialogProps): ReactNode {
  const { t } = useTranslation();
  return (
    <Dialog open={open} title={t("chat.sources.title")} onClose={onClose} className="max-w-xl">
      <ul data-testid="source-list" className="flex max-h-96 flex-col gap-1.5 overflow-y-auto">
        {sources.map((source, index) => {
          const disabled = source.kind === "cli_agent" || source.target === "";
          return (
            <li
              key={`${source.target}-${index}`}
              className="flex items-center justify-between gap-3 rounded-md bg-surface px-2.5 py-1.5"
            >
              <div className="flex min-w-0 flex-col">
                <span className="flex items-center gap-1.5 text-sm text-ink">
                  <span className="rounded-sm bg-surface-overlay px-1 py-0.5 text-[10px] uppercase text-ink-muted">
                    {t(kindLabelKey(source.kind))}
                  </span>
                  <span className="truncate font-medium">{source.label}</span>
                </span>
                <span className="truncate text-xs text-ink-muted" title={source.target}>
                  {source.target}
                </span>
              </div>
              <button
                type="button"
                disabled={disabled}
                aria-label={t("chat.sources.open")}
                onClick={() => onNavigate(source)}
                className="shrink-0 rounded bg-surface-overlay px-2.5 py-1 text-xs text-ink-accent transition-colors hover:bg-surface-raised disabled:cursor-not-allowed disabled:opacity-40"
              >
                {t("chat.sources.open")}
              </button>
            </li>
          );
        })}
      </ul>
    </Dialog>
  );
}
