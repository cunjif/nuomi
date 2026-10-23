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
/** Stream-liveness watchdog tick. */
const LIVENESS_TICK_MS = 500;
/** A delta older than this (and the turn still pending) means the stream
 *  stalled — CLI agents fall silent for seconds between stdout lines while
 *  the agent framework boots or tools run, and LLMs pause between tool
 *  hops. The pill returns for the stall instead of staying dead. */
const DELTA_IDLE_MS = 2_000;
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
  /** Epoch-ms of the last session.delta this turn — the stream-liveness
   *  signal. `undefined` = caller did not wire liveness: the legacy
   *  first-token latch applies (once text arrived the pill stays hidden
   *  for the rest of the turn). CLI-bound conversations MUST wire this —
   *  their first stdout line can land inside the 300ms render window and
   *  the legacy latch would kill the pill before it ever showed. */
  lastDeltaAt?: number | null;
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
  lastDeltaAt,
}: ChatStatusPillProps): ReactNode {
  const { t } = useTranslation();
  const [visible, setVisible] = useState(false);
  const [closing, setClosing] = useState(false);
  const [degraded, setDegraded] = useState(false);
  const [startedAt, setStartedAt] = useState(0);
  const [pillEntries, setPillEntries] = useState<StatusPillEntry[]>([]);
  const [now, setNow] = useState(() => Date.now());

  const phaseStatus = useStatusPhase(isPending, hasStreamText);
  const copyKey = phaseStatus?.copyKey;

  const entriesRef = useRef(entries);
  entriesRef.current = entries;
  const wasPendingRef = useRef(false);
  /** Latched once the reply started this turn — without stream-liveness
   *  wiring this is a hard latch (spec 5.8.1-6a: no new pill until the
   *  next turn, even when the durable message briefly clears
   *  hasStreamText); with liveness wiring the idle watchdog below owns
   *  the return decision, so the latch only stops the legacy path. */
  const firstTokenRef = useRef(false);
  const consumedToolIdsRef = useRef<Set<string>>(new Set());
  const lastSignalAtRef = useRef(0);
  const showTimerRef = useRef<number | null>(null);
  const hideTimerRef = useRef<number | null>(null);
  /** Latest delta timestamp already counted as a degradation reset. */
  const lastDeltaSeenRef = useRef(0);

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

  const hideNow = (): void => {
    clearTimers();
    setVisible(false);
    setClosing(false);
  };

  const scheduleShow = (): void => {
    if (showTimerRef.current !== null) return;
    showTimerRef.current = window.setTimeout(() => setVisible(true), STATUS_RENDER_DELAY_MS);
  };

  // Turn transitions: reset on start, tear down on end (the success path
  // already faded out on the first token; this covers empty/failed turns).
  useEffect(() => {
    if (isPending && !wasPendingRef.current) {
      const nowMs = Date.now();
      firstTokenRef.current = false;
      lastDeltaSeenRef.current = 0;
      wasActiveRef.current = false;
      consumedToolIdsRef.current = new Set(
        entriesRef.current
          .filter((e) => e.kind === "tool_call")
          .map((e) => e.id),
      );
      lastSignalAtRef.current = nowMs;
      setStartedAt(nowMs);
      setDegraded(false);
      setClosing(false);
      setVisible(false);
      setPillEntries([{ text: t("chat.status.pill.initial"), at: nowMs }]);
    } else if (!isPending && wasPendingRef.current) {
      clearTimers();
      setVisible(false);
      setClosing(false);
      setPillEntries([]);
    }
    wasPendingRef.current = isPending;
  }, [isPending, t]);

  // Track the latest delta (a liveness signal AND a degradation reset).
  useEffect(() => {
    if (lastDeltaAt !== undefined && lastDeltaAt !== null && lastDeltaAt > lastDeltaSeenRef.current) {
      lastDeltaSeenRef.current = lastDeltaAt;
      lastSignalAtRef.current = lastDeltaAt;
      setDegraded(false);
    }
  }, [lastDeltaAt]);

  // First stream token: latch the turn. The fade itself is driven by the
  // liveness effect below (streamActive flips); this keeps the latch for
  // the legacy (unwired) path and cancels a pending show so very fast
  // first lines never flash the pill.
  useEffect(() => {
    if (!hasStreamText || firstTokenRef.current) return;
    firstTokenRef.current = true;
    if (showTimerRef.current !== null) {
      clearTimeout(showTimerRef.current);
      showTimerRef.current = null;
    }
  }, [hasStreamText]);

  // Liveness clock: tick while the turn is in flight so an idle stream
  // is detected even without new render inputs.
  useEffect(() => {
    if (!isPending) return;
    const timer = setInterval(() => setNow(Date.now()), LIVENESS_TICK_MS);
    return () => clearInterval(timer);
  }, [isPending]);

  // Stream-liveness, derived reactively from the props (NOT a ref — the
  // render must see fresh values). A delta from BEFORE this turn started
  // does not count: back-to-back turns must not let the previous turn's
  // tail delta kill the new turn's pill.
  const livenessWired = lastDeltaAt !== undefined;
  const deltaThisTurn =
    lastDeltaAt != null && startedAt !== 0 && lastDeltaAt >= startedAt ? lastDeltaAt : null;
  const streamActive = livenessWired
    ? deltaThisTurn !== null && now - deltaThisTurn <= DELTA_IDLE_MS
    : hasStreamText;

  // Falling edge (active → stalled): seed a still-working line so the
  // returning pill explains itself (first appearance keeps the initial
  // copy seeded at turn start).
  const wasActiveRef = useRef(false);
  useEffect(() => {
    if (!isPending) {
      wasActiveRef.current = false;
      return;
    }
    if (!streamActive && wasActiveRef.current) {
      const text = t("chat.status.pill.stillWorking");
      setPillEntries((prev) => {
        if (prev[prev.length - 1]?.text === text) return prev;
        return [...prev, { text, at: Date.now() }];
      });
    }
    wasActiveRef.current = streamActive;
  }, [streamActive, isPending, t]);

  // Visibility state machine (single owner): teardown on turn end / error /
  // stop; fade out while the stream is active; schedule a (re)show while
  // the stream is idle — that return covers CLI stdout stalls (agent boot,
  // tool runs) and LLM inter-tool pauses alike.
  useEffect(() => {
    if (!isPending || hasError || stopped) {
      hideNow();
      return;
    }
    if (streamActive) {
      clearTimers();
      setClosing(true);
      hideTimerRef.current = window.setTimeout(() => {
        setVisible(false);
        setClosing(false);
      }, FADE_OUT_MS);
      return;
    }
    if (!livenessWired && firstTokenRef.current) {
      // Legacy latch: reply started, liveness not wired — stay hidden.
      hideNow();
      return;
    }
    setClosing(false);
    scheduleShow();
  }, [isPending, streamActive, hasError, stopped, livenessWired]);

  // Agent failure hides the pill immediately (spec 5.8.3-1); user stop does
  // the same (spec 5.8.1-5) — the view shows the terminal TurnNotice.
  // (Covered by the visibility effect above via hasError/stopped inputs.)

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
