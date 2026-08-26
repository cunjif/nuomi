/**
 * Relative-time formatting for session lists: `Intl.RelativeTimeFormat`
 * for sub-day spans, plain `M.D` date beyond that (Intl covers the user-
 * facing strings, so no i18n resources are needed here). Pure — `nowMs`
 * is injected so callers and tests stay deterministic.
 */

export type RelativeTimeLocale = "zh-CN" | "en";

const MINUTE_MS = 60_000;
const HOUR_MS = 3_600_000;
const DAY_MS = 86_400_000;

export function formatRelativeTime(
  createdAtMs: number,
  nowMs: number,
  locale: RelativeTimeLocale,
): string {
  const elapsed = nowMs - createdAtMs;
  if (elapsed < MINUTE_MS) {
    return locale === "zh-CN" ? "刚刚" : "just now";
  }
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "always" });
  if (elapsed < HOUR_MS) {
    return rtf.format(-Math.floor(elapsed / MINUTE_MS), "minute");
  }
  if (elapsed < DAY_MS) {
    return rtf.format(-Math.floor(elapsed / HOUR_MS), "hour");
  }
  const created = new Date(createdAtMs);
  return `${created.getMonth() + 1}.${created.getDate()}`;
}
