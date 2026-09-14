import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../../lib/ipc/client";

export interface RunHistoryStripProps {
  sessionId: string;
}

/** History of triggered runs for a scheduled conversation. */
export function RunHistoryStrip({ sessionId }: RunHistoryStripProps): ReactNode {
  const { t } = useTranslation();
  const eventsQuery = useQuery({
    queryKey: ["sessionEvents", sessionId, "scheduled"],
    queryFn: () => ipc.listEvents(sessionId, 0),
    refetchInterval: 10_000,
  });

  const events = eventsQuery.data ?? [];
  const runs = events.filter((e) => e.kind === "state_changed");

  if (runs.length === 0) {
    return <p className="py-2 text-xs text-ink-muted">{t("conversation.noRunHistory")}</p>;
  }

  return (
    <div className="flex gap-1 overflow-x-auto py-1">
      {runs.map((run) => (
        <div
          key={run.seq}
          className="shrink-0 rounded border border-ink-muted/20 px-1.5 py-0.5 text-xs text-ink-muted"
        >
          {new Date(run.createdAt).toLocaleTimeString()}
        </div>
      ))}
    </div>
  );
}
