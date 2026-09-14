import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { AttachmentDto } from "../../../lib/ipc/client";
import { formatSize } from "../../../lib/conversation/attachmentModel";

export interface AttachmentShelfProps {
  attachments: AttachmentDto[];
  onRemove: (id: string) => void;
}

/**
 * Horizontal scrolling attachment list rendered above the composer textarea.
 * Images show a thumbnail; other files show an icon + name + size.
 * Each item has a delete button visible on hover.
 */
export function AttachmentShelf({ attachments, onRemove }: AttachmentShelfProps): ReactNode {
  const { t } = useTranslation();

  if (attachments.length === 0) return null;

  return (
    <div
      className="flex gap-1 overflow-x-auto pb-1"
      role="list"
      aria-label={t("composer.attachmentShelfLabel")}
    >
      {attachments.map((att) => (
        <div
          key={att.id}
          role="listitem"
          className="group relative flex shrink-0 items-center gap-1 rounded border border-ink-muted/30 bg-surface-raised px-1.5 py-1 text-xs text-ink"
        >
          {att.kind === "image" ? (
            <img
              src={`.nuomi/attachments/${att.sessionId}/${att.sha256}.${att.name.split(".").pop() ?? "png"}`}
              alt={att.name}
              className="h-8 w-8 rounded object-cover"
            />
          ) : (
            <span aria-hidden="true" className="text-sm">📎</span>
          )}
          <span className="max-w-32 truncate">{att.name}</span>
          <span className="text-ink-muted">{formatSize(att.sizeBytes)}</span>
          <button
            type="button"
            onClick={() => onRemove(att.id)}
            className="ml-0.5 text-ink-muted opacity-0 transition-opacity hover:text-ink group-hover:opacity-100 focus-visible:opacity-100"
            aria-label={t("composer.attachmentRemove")}
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
