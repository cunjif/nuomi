import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { STATUS_RENDER_DELAY_MS, useStatusPhase } from "./statusPhase";
import { StatusPill, type StatusPillEntry } from "./StatusPill";
import type { ChatEntry } from "./useSessionStream";

/** No real agent signal for this long → degrade to rotating fun copy
 *  (spec 5.8.3-2). */
const SIGNAL_TIMEOUT_MS = 10_000;
/** StatusPill fade-out length — the pill unmounts after the transition. */
const FADE_OUT_MS = 200;
const TARGET_SUMMARY_MAX = 24;

export interface ChatStatusPillProps {
  isPending: boolean;
  hasStreamText: boolean;
  hasError: boolean;
  /** Chat entries feed — tool_call entries arriving during the turn are
   *  real agent signals that update the pill within 1s (spec 5.8.1-2a). */
  entries: ChatEntry[];
  /** User pressed stop this turn (spec 5.8.1-5): hide immediately; the
   *  view renders the "stopped" TurnNotice. */
  stopped?: boolean;
  /** Stop affordance rendered inside the pill (spec 5.8.1-5a). */
  onStop?: () => void;
}

/** First string-typed argument as a short target summary, or null when the
 *  args carry no usable string (malformed JSON included — the bare tool
 *  name is still an accurate status). */
function toolTargetSummary(argsJson: string | undefined): string | null {
  if (argsJson === undefined) return null;
  try {
    const parsed: unknown = JSON.parse(argsJson);
    if (typeof parsed !== "object" || parsed === null) return null;
    for (const value of Object.values(parsed as Record<string, unknown>)) {
      if (typeof value === "string" && value.trim().length > 0) {
        const trimmed = value.trim();
        return trimmed.length > TARGET_SUMMARY_MAX
          ? `${trimmed.slice(0, TARGET_SUMMARY_MAX)}…`
          : trimmed;
      }
    }
  } catch {
    // malformed args — fall back to the tool-name-only status
  }
  return null;
}

/**
 * Single-chat StatusPill wiring (spec 5.8.1-1/2/3, task 8.2): turns the
 * pending turn + session stream into pill entries — real tool.call signals
 * take priority, 10s without any signal degrades to the rotating fun copy
 * (useStatusPhase), and the first stream token latches the turn shut: the
 * pill fades out and never reappears until the next turn.
 */
export function ChatStatusPill({
  isPending,
  hasStreamText,
  hasError,
  entries,
  stopped = false,
  onStop,
}: ChatStatusPillProps): ReactNode {
  const { t } = useTranslation();
  const [visible, setVisible] = useState(false);
  const [closing, setClosing] = useState(false);
  const [degraded, setDegraded] = useState(false);
  const [startedAt, setStartedAt] = useState(0);
  const [pillEntries, setPillEntries] = useState<StatusPillEntry[]>([]);

  const phaseStatus = useStatusPhase(isPending, hasStreamText);
  const copyKey = phaseStatus?.copyKey;

  const entriesRef = useRef(entries);
  entriesRef.current = entries;
  const wasPendingRef = useRef(false);
  /** Latched once the reply started this turn — no new pill until the next
   *  turn (spec 5.8.1-6a), even when the durable message briefly clears
   *  hasStreamText while the turn is still pending. */
  const firstTokenRef = useRef(false);
  const consumedToolIdsRef = useRef<Set<string>>(new Set());
  const lastSignalAtRef = useRef(0);
  const showTimerRef = useRef<number | null>(null);
  const hideTimerRef = useRef<number | null>(null);

  const clearTimers = (): void => {
    if (showTimerRef.current !== null) {
      clearTimeout(showTimerRef.current);
      showTimerRef.current = null;
    }
    if (hideTimerRef.current !== null) {
      clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
  };

  // Turn transitions: reset on start, tear down on end (the success path
  // already faded out on the first token; this covers empty/failed turns).
  useEffect(() => {
    if (isPending && !wasPendingRef.current) {
      const now = Date.now();
      firstTokenRef.current = false;
      consumedToolIdsRef.current = new Set(
        entriesRef.current
          .filter((e) => e.kind === "tool_call")
          .map((e) => e.id),
      );
      lastSignalAtRef.current = now;
      setStartedAt(now);
      setDegraded(false);
      setClosing(false);
      setVisible(false);
      setPillEntries([{ text: t("chat.status.pill.initial"), at: now }]);
      showTimerRef.current = window.setTimeout(
        () => setVisible(true),
        STATUS_RENDER_DELAY_MS,
      );
    } else if (!isPending && wasPendingRef.current) {
      clearTimers();
      setVisible(false);
      setClosing(false);
      setPillEntries([]);
    }
    wasPendingRef.current = isPending;
  }, [isPending, t]);

  // First stream token: latch the turn, cancel the pending show (very fast
  // responses never flash the pill), fade the visible pill out.
  useEffect(() => {
    if (!hasStreamText || firstTokenRef.current) return;
    firstTokenRef.current = true;
    if (showTimerRef.current !== null) {
      clearTimeout(showTimerRef.current);
      showTimerRef.current = null;
    }
    setClosing(true);
    hideTimerRef.current = window.setTimeout(() => {
      setVisible(false);
      setClosing(false);
    }, FADE_OUT_MS);
  }, [hasStreamText]);

  // Agent failure hides the pill immediately (spec 5.8.3-1); user stop does
  // the same (spec 5.8.1-5) — the view shows the terminal TurnNotice.
  useEffect(() => {
    if (!hasError && !stopped) return;
    clearTimers();
    setVisible(false);
    setClosing(false);
  }, [hasError, stopped]);

  // Real signals: tool_call entries arriving during the turn. History
  // tool_calls are pre-consumed at turn start so only fresh calls count.
  useEffect(() => {
    if (!isPending || firstTokenRef.current) return;
    const fresh = entries.filter(
      (e) => e.kind === "tool_call" && !consumedToolIdsRef.current.has(e.id),
    );
    if (fresh.length === 0) return;
    const now = Date.now();
    const additions: StatusPillEntry[] = [];
    for (const e of fresh) {
      consumedToolIdsRef.current.add(e.id);
      const target = toolTargetSummary(e.argsJson);
      additions.push({
        text:
          target !== null
            ? t("chat.status.toolCallWithTarget", { tool: e.tool ?? "?", target })
            : t("chat.status.toolCall", { tool: e.tool ?? "?" }),
        at: now,
      });
    }
    setPillEntries((prev) => {
      const next = [...prev];
      for (const a of additions) {
        // Consecutive-duplicate guard: the same call can surface as a live
        // entry (live-cN) and again as a persisted one (cM) after a gap
        // refetch — one status line is enough.
        if (next[next.length - 1]?.text !== a.text) next.push(a);
      }
      return next;
    });
    lastSignalAtRef.current = now;
    setDegraded(false);
  }, [entries, isPending, t]);

  // Degradation watchdog: 10s without a real signal → fun-copy rotation.
  useEffect(() => {
    if (!isPending) return;
    const timer = setInterval(() => {
      if (Date.now() - lastSignalAtRef.current >= SIGNAL_TIMEOUT_MS) {
        setDegraded(true);
      }
    }, 1000);
    return () => clearInterval(timer);
  }, [isPending]);

  // While degraded, every fun-copy rotation becomes a new pill entry.
  useEffect(() => {
    if (!degraded || copyKey === undefined) return;
    const text = t(copyKey);
    setPillEntries((prev) => {
      const last = prev[prev.length - 1];
      if (last?.text === text) return prev;
      return [...prev, { text, at: Date.now() }];
    });
  }, [degraded, copyKey, t]);

  useEffect(() => () => clearTimers(), []);

  if (!visible || pillEntries.length === 0) return null;

  return (
    <StatusPill entries={pillEntries} startedAt={startedAt} closing={closing} onStop={onStop} />
  );
}
