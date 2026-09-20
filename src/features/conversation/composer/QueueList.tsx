import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ipc } from "../../../lib/ipc/client";

interface QueueListProps {
  sessionId: string;
}

/** ADR 0015: Renders queued messages above the composer input. */
export function QueueList({ sessionId }: QueueListProps): ReactNode {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const queueQuery = useQuery({
    queryKey: ["messageQueue", sessionId],
    queryFn: () => ipc.listMessageQueue(sessionId),
  });
  const cancelMut = useMutation({
    mutationFn: (id: string) => ipc.cancelMessageQueueItem(id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ["messageQueue", sessionId] }),
  });
  const items = queueQuery.data ?? [];
  if (items.length === 0) return null;
  return (
    <div className="flex flex-col gap-1 px-3 py-2">
      <p className="text-xs font-semibold text-ink-muted">
        {t("chat.queueTitle", { count: items.length })}
      </p>
      {items.map((item, idx) => (
        <div
          key={item.id}
          className="flex items-center gap-2 rounded-md border border-dashed border-ink-muted/30 bg-surface-raised px-2 py-1 text-xs"
        >
          <span className="shrink-0 text-ink-muted">{idx + 1}.</span>
          <span className="flex-1 truncate text-ink">{item.text}</span>
          <button
            type="button"
            onClick={() => cancelMut.mutate(item.id)}
            className="shrink-0 text-ink-muted hover:text-state-danger"
            aria-label={t("chat.cancelQueueItem")}
          >
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
