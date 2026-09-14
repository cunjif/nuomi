import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../../lib/ipc/client";
import { useUiStore } from "../../../lib/store/uiStore";
import { ScheduleRuleCard } from "./ScheduleRuleCard";
import { NextRunCountdown } from "./NextRunCountdown";
import { RunHistoryStrip } from "./RunHistoryStrip";

/**
 * Scheduled conversation view: rule card + countdown + run history.
 */
export function ScheduledConversationView(): ReactNode {
  const { t } = useTranslation();
  const sessionId = useUiStore((s) => s.selectedSessionId);

  const schedulesQuery = useQuery({
    queryKey: ["schedules"],
    queryFn: () => ipc.listSchedules(),
    staleTime: 30_000,
  });

  if (sessionId === null) {
    return <div className="flex h-full items-center justify-center text-sm text-ink-muted">{t("chat.noSession")}</div>;
  }

  const schedules = schedulesQuery.data ?? [];

  return (
    <div className="flex h-full flex-col p-2">
      {schedules.length > 0 && <ScheduleRuleCard schedule={schedules[0]!} />}
      <div className="py-2">
        <NextRunCountdown nextTriggerAt={schedules[0]?.nextTriggerAt ?? null} />
      </div>
      <div className="border-t border-ink-muted/30 pt-2">
        <h3 className="mb-1 text-xs font-semibold text-ink-muted">{t("conversation.runHistory")}</h3>
        <RunHistoryStrip sessionId={sessionId} />
      </div>
    </div>
  );
}
