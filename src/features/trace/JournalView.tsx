import type { ReactNode } from "react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { AsyncBoundary } from "../../components/ui/AsyncBoundary";
import { useStickToBottom } from "../../components/ui/useStickToBottom";
import { ipc } from "../../lib/ipc/client";
import type { JsonValue } from "../../lib/ipc/bindings.gen";
import { asRecord, asString } from "../../lib/events/payload";
import {
  buildJournalEntries,
  invokeJournalRollback,
  JOURNAL_ROLLBACK_WIRED,
  JOURNAL_SESSION_ID,
  latestDriftAlert,
  rolledBackFromSeq,
  type JournalEntryKind,
  type JournalEntryVm,
} from "./traceModel";

/** Per-kind accent colors (state tokens, dark-first theme). */
const KIND_ACCENT: Record<JournalEntryKind, string> = {
  reflection_triggered: "text-ink-accent",
  proposal_generated: "text-ink-accent",
  gate_verdict: "text-state-warn",
  applied: "text-state-ok",
  rolled_back: "text-state-warn",
  drift_alert: "text-state-danger",
  baseline_captured: "text-ink-muted",
  other: "text-ink-muted",
};

/** Harness Journal tab: evolution audit timeline + drift banner + rollback. */
export function JournalView(): ReactNode {
  const { t } = useTranslation();
  // The journal mirror lives under the synthetic `journal` session, served
  // by the existing list_events IPC (no dedicated command needed).
  const journalQuery = useQuery({
    queryKey: ["journalEvents"],
    queryFn: () => ipc.listEvents(JOURNAL_SESSION_ID, 0),
    retry: false,
  });
  const entries = useMemo(
    () => buildJournalEntries(journalQuery.data ?? []),
    [journalQuery.data],
  );
  const drift = useMemo(() => latestDriftAlert(entries), [entries]);
  const revertedSeqs = useMemo(() => {
    const seqs = new Set<number>();
    for (const entry of entries) {
      const from = rolledBackFromSeq(entry);
      if (from !== null) seqs.add(from);
    }
    return seqs;
  }, [entries]);

  const { scrollRef, scrollToBottomIfStuck } = useStickToBottom();

  useEffect(() => {
    scrollToBottomIfStuck();
  }, [entries.length, scrollToBottomIfStuck]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      {drift !== null && (
        <div
          role="alert"
          className="mx-3 mt-2 rounded border border-state-danger bg-surface-raised px-3 py-2 text-xs text-state-danger"
        >
          {t("journal.driftBanner", {
            metric: driftMetric(drift),
            delta: driftDelta(drift),
          })}
        </div>
      )}
      <h3 className="px-3 pt-2 text-xs font-semibold uppercase tracking-wide text-ink-muted">
        {t("journal.heading")}
      </h3>
      <div className="min-h-0 flex-1">
        <AsyncBoundary
          isLoading={journalQuery.isLoading}
          // The mirror session is created lazily by the backend; a missing
          // sink reads as "no journal yet", not as an error.
          error={null}
          isEmpty={entries.length === 0}
          emptyLabel={t("journal.empty")}
          onRetry={() => void journalQuery.refetch()}
        >
          <div ref={scrollRef} className="h-full overflow-y-auto py-1">
            {entries.map((entry) => (
              <JournalRow
                key={`${entry.seq}-${entry.ts}`}
                entry={entry}
                isReverted={revertedSeqs.has(entry.seq)}
              />
            ))}
          </div>
        </AsyncBoundary>
      </div>
    </div>
  );
}

function asNumber(value: JsonValue | undefined): number | null {
  return typeof value === "number" ? value : null;
}

function driftMetric(entry: JournalEntryVm): string {
  return asString(asRecord(entry.payload)?.metric) || "-";
}

function driftDelta(entry: JournalEntryVm): string {
  const delta = asNumber(asRecord(entry.payload)?.delta);
  return delta === null ? "-" : String(delta);
}

function JournalRow({ entry, isReverted }: { entry: JournalEntryVm; isReverted: boolean }): ReactNode {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const fromSeq = rolledBackFromSeq(entry);
  const highlight =
    entry.kind === "rolled_back" || isReverted
      ? "border-state-warn/60 bg-state-warn/5"
      : "border-ink-muted/40";
  return (
    <div className={`mx-3 my-1 rounded border ${highlight} bg-surface-raised px-3 py-1.5 text-sm`}>
      <div className="flex items-center gap-2">
        <span className={`text-xs font-semibold ${KIND_ACCENT[entry.kind]}`}>
          {t(`journal.kind.${entry.kind}`)}
        </span>
        <span className="font-mono text-[10px] text-ink-muted">#{entry.seq}</span>
        <span className="text-xs text-ink-muted">{entry.actor}</span>
        <span className="ml-auto text-[10px] text-ink-muted">
          {new Date(entry.ts).toLocaleString()}
        </span>
      </div>
      <p className="break-words text-sm text-ink">{entry.summary}</p>
      {fromSeq !== null && (
        <p className="text-xs text-state-warn">{t("journal.rolledBackFrom", { seq: fromSeq })}</p>
      )}
      {entry.kind === "applied" && (
        <button
          type="button"
          disabled={!JOURNAL_ROLLBACK_WIRED}
          title={
            JOURNAL_ROLLBACK_WIRED
              ? t("journal.rollback")
              : t("journal.rollbackPending")
          }
          onClick={() => void invokeJournalRollback(entry.seq)}
          className="mt-1 rounded border border-state-danger px-2 py-0.5 text-xs text-state-danger hover:bg-surface focus-visible:ring-2 focus-visible:ring-ink-accent disabled:opacity-50"
        >
          {t("journal.rollback")}
        </button>
      )}
      {(entry.evidenceRefs.length > 0 || entry.payload !== null) && (
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          className="mt-1 text-xs text-ink-muted focus-visible:ring-2 focus-visible:ring-ink-accent"
        >
          {open ? t("journal.hideDetail") : t("journal.showDetail")}
        </button>
      )}
      {open && (
        <div className="mt-1 space-y-1">
          {entry.evidenceRefs.length > 0 && (
            <p className="text-xs text-ink-muted">
              {t("journal.evidence")}:{" "}
              {entry.evidenceRefs.map((ref) => (
                <span key={ref} className="mr-1 font-mono text-ink-accent">
                  {ref}
                </span>
              ))}
            </p>
          )}
          <pre className="max-h-40 overflow-auto rounded bg-surface p-2 font-mono text-xs text-ink-muted">
            {JSON.stringify(entry.payload, null, 2)}
          </pre>
        </div>
      )}
    </div>
  );
}
