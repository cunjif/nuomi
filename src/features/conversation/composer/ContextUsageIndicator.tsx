import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useQuery } from "@tanstack/react-query";
import { ipc } from "../../../lib/ipc/client";

function formatTokens(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return String(n);
}

function formatTime(ms: number): string {
  return new Date(ms).toLocaleString();
}

/**
 * Context usage indicator + prune popup (steward task 4.6, spec 5.3.1-3,
 * generalized to every composer via the session-generic `getContextUsage`
 * IPC): a compact token-usage ratio bar in the composer area; clicking
 * opens a popup with the per-section breakdown and the prune history.
 * The DTO carries usage numbers only — no secrets ever enter this
 * surface (spec 5.3.1-5a).
 */
export function ContextUsageIndicator({
  sessionId,
}: {
  sessionId: string | null;
}): ReactNode {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement | null>(null);

  const { data } = useQuery({
    queryKey: ["contextUsage", sessionId],
    queryFn: () => {
      if (sessionId === null) throw new Error("no session");
      return ipc.getContextUsage(sessionId);
    },
    enabled: sessionId !== null,
    staleTime: 10_000,
  });

  // Outside-click dismissal (spec 5.3.1-3c: click outside → close, no
  // residual state).
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent): void => {
      if (rootRef.current !== null && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
      }
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  if (sessionId === null || data === undefined) return null;

  const ratio = data.budgetTokens > 0 ? data.totalTokens / data.budgetTokens : 0;
  const overBudget = ratio > 1;

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        aria-label={t("composer.contextUsage")}
        title={t("composer.contextUsageTitle")}
        onClick={() => setOpen((v) => !v)}
        className="flex w-44 items-center gap-2 rounded px-1 py-0.5 text-xs text-ink-muted hover:bg-surface"
      >
        <span className="h-1.5 flex-1 overflow-hidden rounded bg-surface-raised">
          <span
            className={`block h-full ${overBudget ? "bg-danger" : "bg-ink-accent"}`}
            style={{ width: `${Math.min(100, ratio * 100)}%` }}
          />
        </span>
        <span className="whitespace-nowrap">
          {formatTokens(data.totalTokens)} / {formatTokens(data.budgetTokens)}
        </span>
      </button>
      {open && (
        <div
          role="dialog"
          aria-label={t("composer.contextUsagePopupLabel")}
          className="absolute bottom-6 right-0 z-30 w-80 rounded border border-ink-muted/30 bg-surface-raised p-3 text-sm text-ink shadow-lg"
        >
          <p className="mb-2 font-medium">{t("composer.contextUsageSections")}</p>
          <ul className="mb-2 space-y-1">
            {data.sections.map((s) => (
              <li key={s.name} className="flex justify-between text-xs">
                <span>{s.name}</span>
                <span className="text-ink-muted">
                  {t("composer.contextUsageSectionRow", {
                    tokens: s.tokens,
                    percent: Math.round((s.tokens / Math.max(1, data.totalTokens)) * 100),
                  })}
                </span>
              </li>
            ))}
          </ul>
          <p className="mb-1 text-xs text-ink-muted">
            {t("composer.contextUsageTotal", {
              total: data.totalTokens,
              budget: data.budgetTokens,
              percent: Math.round(ratio * 100),
            })}
          </p>
          <div className="border-t border-ink-muted/20 pt-2">
            <p className="mb-1 text-xs font-medium">{t("composer.contextUsagePruneTitle")}</p>
            {data.pruneRecords.length === 0 ? (
              <p className="text-xs text-ink-muted">{t("composer.contextUsageNoPrune")}</p>
            ) : (
              <ul className="max-h-40 space-y-1 overflow-y-auto text-xs text-ink-muted">
                {data.pruneRecords.map((r, i) => (
                  <li key={i}>
                    {t("composer.contextUsagePruneItem", {
                      time: formatTime(r.occurredAt),
                      folded: r.foldedMessages,
                      dedup: r.duplicatesCollapsed,
                      summarized: r.resultsSummarized,
                      tokens: r.approxTokens,
                    })}
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
