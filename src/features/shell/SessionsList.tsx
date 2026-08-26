import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { ipc } from "../../lib/ipc/client";
import { describeError } from "../../i18n";
import { toast } from "../../lib/store/toastStore";
import { useUiStore } from "../../lib/store/uiStore";
import { formatRelativeTime, type RelativeTimeLocale } from "../../lib/format/relativeTime";

/** Session list with create + resume (select) actions. */
export function SessionsList(): ReactNode {
  const { t, i18n } = useTranslation();
  const qc = useQueryClient();
  const selectedSessionId = useUiStore((s) => s.selectedSessionId);
  const selectSession = useUiStore((s) => s.selectSession);
  // Polling keeps sessions created by the other surface (CLI shares the
  // same SQLite db) visible without a manual refresh.
  const sessionsQuery = useQuery({
    queryKey: ["sessions"],
    queryFn: ipc.listSessions,
    refetchInterval: 15_000,
  });
  const locale: RelativeTimeLocale = i18n.language === "en" ? "en" : "zh-CN";

  const createMut = useMutation({
    mutationFn: ipc.createSession,
    onSuccess: (session) => {
      void qc.invalidateQueries({ queryKey: ["sessions"] });
      selectSession(session.id);
    },
    onError: (e) => toast.error(describeError(e)),
  });
  const resumeMut = useMutation({
    mutationFn: ipc.resumeSession,
    onError: (e) => toast.error(`${t("sessions.resumeFailed")}: ${describeError(e)}`),
  });

  return (
    <section aria-label={t("sessions.heading")} className="flex h-full flex-col p-2">
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-xs font-semibold uppercase tracking-wide text-ink-muted">{t("sessions.heading")}</h2>
        <button
          type="button"
          onClick={() => createMut.mutate()}
          disabled={createMut.isPending}
          className="rounded bg-ink-accent px-2 py-0.5 text-xs text-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
        >
          {t("sessions.newSession")}
        </button>
      </div>
      <AsyncBoundary
        isLoading={sessionsQuery.isLoading}
        error={sessionsQuery.error}
        isEmpty={(sessionsQuery.data?.length ?? 0) === 0}
        onRetry={() => void sessionsQuery.refetch()}
      >
        <ul className="flex flex-col gap-0.5">
          {(sessionsQuery.data ?? []).map((session) => (
            <li key={session.id}>
              <button
                type="button"
                onClick={() => {
                  selectSession(session.id);
                  resumeMut.mutate(session.id);
                }}
                aria-current={selectedSessionId === session.id ? "true" : undefined}
                title={session.title}
                className={`flex w-full items-baseline gap-2 rounded px-2 py-1 text-left text-sm focus-visible:ring-2 focus-visible:ring-ink-accent ${
                  selectedSessionId === session.id
                    ? "bg-surface-overlay text-ink"
                    : "text-ink-muted hover:bg-surface-overlay"
                }`}
              >
                <span className="min-w-0 flex-1 truncate">{session.title}</span>
                <span className="shrink-0 text-xs tabular-nums">
                  {formatRelativeTime(session.createdAt, Date.now(), locale)}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </AsyncBoundary>
    </section>
  );
}
