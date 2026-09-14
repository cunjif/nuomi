import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../../lib/ipc/client";
import { useUiStore } from "../../../lib/store/uiStore";
import { RunStatusBar } from "./RunStatusBar";
import { RunTimeline } from "./RunTimeline";

/**
 * Background task conversation view: run status bar + timeline + composer.
 */
export function BackgroundConversationView(): ReactNode {
  const { t } = useTranslation();
  const sessionId = useUiStore((s) => s.selectedSessionId);

  const eventsQuery = useQuery({
    queryKey: ["sessionEvents", sessionId, "background"],
    queryFn: () => (sessionId ? ipc.listEvents(sessionId, 0) : Promise.resolve([])),
    enabled: sessionId !== null,
    refetchInterval: 3_000,
  });
  const runsQuery = useQuery({
    queryKey: ["activeRuns", sessionId],
    queryFn: () => ipc.listActiveRuns(),
    refetchInterval: 3_000,
  });

  if (sessionId === null) {
    return <div className="flex h-full items-center justify-center text-sm text-ink-muted">{t("chat.noSession")}</div>;
  }

  const activeRun = (runsQuery.data ?? []).find((r) => r.sessionId === sessionId) ?? null;

  return (
    <div className="flex h-full flex-col">
      <RunStatusBar run={activeRun} onStop={() => void ipc.stopConversation(sessionId)} />
      <RunTimeline events={eventsQuery.data ?? []} />
    </div>
  );
}
