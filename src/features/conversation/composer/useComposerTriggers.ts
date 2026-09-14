import { useMemo } from "react";

/** Which completion mode the composer is in. */
export type TriggerMode = "command" | "mention" | null;

export interface TriggerState {
  /** Active completion mode. */
  mode: TriggerMode;
  /** The query text after the trigger character (e.g. "age" for "/age"). */
  query: string;
  /** Character index where the trigger starts (for replacement on select). */
  start: number;
  /** Character index where the query ends (caret position). */
  end: number;
  /** The namespace for `@` triggers: "file", "agent", "session", or null (all). */
  namespace: string | null;
}

/**
 * Detects `/` (command) and `@` (mention) triggers in the composer text.
 *
 * `/` triggers at the start of the input (no preceding whitespace).
 * `@` triggers at word boundaries (start of line or after whitespace).
 * The `@` namespace is parsed from the text after `@`: `@file`, `@agent`, etc.
 *
 * Returns null when no trigger is active.
 */
export function detectTrigger(value: string, caret: number): TriggerState | null {
  if (caret === 0 || caret > value.length) return null;

  // Command mode: `/` at position 0, no whitespace yet.
  if (value.startsWith("/")) {
    const before = value.slice(0, caret);
    if (/\s/.test(before)) return null;
    return {
      mode: "command",
      query: before.slice(1).toLowerCase(),
      start: 0,
      end: caret,
      namespace: null,
    };
  }

  // Mention mode: `@` at a word boundary.
  const before = value.slice(0, caret);
  const atMatch = before.match(/(?:^|\s)@(\w*)$/);
  if (atMatch) {
    const atStart = before.length - atMatch[0].length + (atMatch[0].startsWith(" ") ? 1 : 0);
    const afterAt = atMatch[1] ?? "";
    const nsMatch = afterAt.match(/^(\w+):/);
    const ns = nsMatch?.[1] ?? null;
    return {
      mode: "mention",
      query: nsMatch ? afterAt.slice((nsMatch[1] ?? "").length + 1) : afterAt,
      start: atStart,
      end: caret,
      namespace: ns,
    };
  }

  return null;
}

/** Hook wrapper for detectTrigger — memoized on value + caret. */
export function useComposerTriggers(value: string, caret: number): TriggerState | null {
  return useMemo(() => detectTrigger(value, caret), [value, caret]);
}
