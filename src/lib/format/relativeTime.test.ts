import { describe, expect, it } from "vitest";
import { formatRelativeTime } from "./relativeTime";

const MINUTE_MS = 60_000;
const HOUR_MS = 3_600_000;
const DAY_MS = 86_400_000;
// Fixed "now": 2026-08-26T04:00:00Z.
const NOW = Date.UTC(2026, 7, 26, 4, 0, 0);

describe("formatRelativeTime — sub-minute", () => {
  it.each([
    ["same instant", NOW],
    ["30s ago", NOW - 30_000],
    ["just under a minute", NOW - 59_999],
    // Clock skew (future timestamp) still reads as "just now".
    ["slightly in the future", NOW + 5_000],
  ])("%s → just-now wording", (_name, createdAt) => {
    expect(formatRelativeTime(createdAt, NOW, "zh-CN")).toBe("刚刚");
    expect(formatRelativeTime(createdAt, NOW, "en")).toBe("just now");
  });
});

describe("formatRelativeTime — minutes and hours", () => {
  const CASES: ReadonlyArray<readonly [label: string, elapsedMs: number]> = [
    ["exactly one minute", MINUTE_MS],
    ["five minutes", 5 * MINUTE_MS],
    ["just under an hour", 59 * MINUTE_MS + 59_999],
    ["exactly one hour", HOUR_MS],
    ["twenty-three hours", 23 * HOUR_MS],
  ];

  it.each(CASES)("%s → zh-CN minute/hour unit", (_name, elapsed) => {
    const text = formatRelativeTime(NOW - elapsed, NOW, "zh-CN");
    const [value, unit] =
      elapsed < HOUR_MS
        ? [Math.floor(elapsed / MINUTE_MS), "分钟"]
        : [Math.floor(elapsed / HOUR_MS), "小时"];
    expect(text).toMatch(new RegExp(`^${value}\\s*${unit}前$`));
  });

  it.each(CASES)("%s → en minute/hour unit", (_name, elapsed) => {
    const text = formatRelativeTime(NOW - elapsed, NOW, "en");
    if (elapsed < HOUR_MS) {
      const minutes = Math.floor(elapsed / MINUTE_MS);
      expect(text).toBe(
        `${minutes} minute${minutes === 1 ? "" : "s"} ago`,
      );
    } else {
      const hours = Math.floor(elapsed / HOUR_MS);
      expect(text).toBe(`${hours} hour${hours === 1 ? "" : "s"} ago`);
    }
  });
});

describe("formatRelativeTime — day and beyond", () => {
  it.each([
    ["exactly one day", DAY_MS],
    ["two days ago", 2 * DAY_MS],
    ["a month-ish back", 30 * DAY_MS],
  ])("%s → M.D date", (_name, elapsed) => {
    const createdAt = NOW - elapsed;
    const created = new Date(createdAt);
    const expectedDate = `${created.getMonth() + 1}.${created.getDate()}`;
    expect(formatRelativeTime(createdAt, NOW, "zh-CN")).toBe(expectedDate);
    expect(formatRelativeTime(createdAt, NOW, "en")).toBe(expectedDate);
  });

  it("date branch never contains relative wording", () => {
    const text = formatRelativeTime(NOW - 5 * DAY_MS, NOW, "zh-CN");
    expect(text).not.toMatch(/前|小时|分钟/);
  });
});
