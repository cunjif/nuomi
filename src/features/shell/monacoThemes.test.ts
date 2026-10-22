import { describe, expect, it } from "vitest";
import {
  NUOMI_MONACO_CHALK,
  NUOMI_MONACO_EINK,
  NUOMI_MONACO_GRID,
  NUOMI_MONACO_HIGH,
  NUOMI_MONACO_PAPER,
  NUOMI_THEME_DATA,
  monacoThemeFor,
} from "./monacoThemes";

const THEMES = ["paper-light", "eink-paper", "grid-notebook", "chalkboard-dark", "high-contrast"] as const;

describe("monacoThemes — syntax rules (P2-E)", () => {
  it.each(THEMES)("%s fills every syntax slot with a valid 6-digit hex", (theme) => {
    const rules = NUOMI_THEME_DATA[theme].rules ?? [];
    // 12 slots: comment/keyword/string/number/constant/type/function/
    // variable/tag/attribute.name/attribute.value/metatag.
    expect(rules.length).toBe(12);
    for (const rule of rules) {
      expect(rule.foreground ?? "").toMatch(/^[0-9a-f]{6}$/i);
    }
  });

  it("keeps the token set identical across themes (lockstep structure)", () => {
    const tokensOf = (t: (typeof THEMES)[number]): string[] => (NUOMI_THEME_DATA[t].rules ?? []).map((r) => r.token);
    const baseline = tokensOf("paper-light");
    for (const theme of THEMES.slice(1)) {
      expect(tokensOf(theme)).toEqual(baseline);
    }
  });

  it("maps every app theme to a distinct registered theme name", () => {
    expect(THEMES.map(monacoThemeFor)).toEqual([
      NUOMI_MONACO_PAPER,
      NUOMI_MONACO_EINK,
      NUOMI_MONACO_GRID,
      NUOMI_MONACO_CHALK,
      NUOMI_MONACO_HIGH,
    ]);
    expect(new Set(THEMES.map(monacoThemeFor)).size).toBe(5);
  });
});
