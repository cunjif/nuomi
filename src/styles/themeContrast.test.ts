// Visual QA as a permanent regression gate: parse BOTH palettes from
// src/styles/global.css, resolve every semantic pairing actually used in the
// app (grep-derived, see per-row rationale), and assert WCAG contrast ratios.
// Any future palette tweak that breaks readability fails here.
//
// Thresholds: text pairs ≥ 4.5:1 (WCAG 1.4.3 — all app copy is < 18pt);
// UI component boundaries / indicators ≥ 3:1 (WCAG 1.4.11).
//
// Deliberately excluded (documented so absence is a decision, not an oversight):
// - Alpha hairline borders (`border-ink-muted/30|40|50|60`): supplementary
//   separators whose identity is carried by the text/fills asserted below.
//   Even pure black at ≤ 60% alpha cannot reach 3:1 against these surfaces,
//   so they are decorative by construction (1.4.11 exempts decoration).
// - `disabled:opacity-50` states (explicitly exempted by WCAG).
//
// NOTE on loading: Vitest's disabled-CSS stub empties every `*.css*` module
// (including `./global.css?raw`), so we read the stylesheet from disk instead.
// `node:fs` is imported through an opaque specifier because @types/node is not
// installed; a static import would fail typecheck, the runtime resolves it fine.
import { describe, expect, it } from "vitest";

declare const process: { cwd(): string };

interface Rgb {
  readonly r: number;
  readonly g: number;
  readonly b: number;
}
type Rgba = Rgb & { readonly a: number };

type FsLike = { readFileSync: (filePath: string, options: { encoding: "utf8" }) => string };

async function readGlobalCss(): Promise<string> {
  const builtinId = ["node", "fs"].join(":");
  const fs = (await import(/* @vite-ignore */ builtinId)) as unknown as FsLike;
  return fs.readFileSync(`${process.cwd()}/src/styles/global.css`, { encoding: "utf8" });
}

const TEXT_MIN = 4.5;
const UI_MIN = 3;

const TOKEN_VARS = [
  "surface",
  "surface-raised",
  "surface-overlay",
  "scrim",
  "ink",
  "ink-muted",
  "accent",
  "ok",
  "warn",
  "danger",
] as const;
type TokenVar = (typeof TOKEN_VARS)[number];
// resolveTheme stores vars camelCased via `toCamel` (surface-raised →
// surfaceRaised), so the resolved shape must mirror that, not the raw names.
type Camel<S extends string> = S extends `${infer Head}-${infer Rest}`
  ? `${Head}${Capitalize<Camel<Rest>>}`
  : S;
type ResolvedTheme = {
  [K in TokenVar as Camel<K>]: K extends "scrim" ? Rgba : Rgb;
};

function extractBlock(css: string, selector: string): string {
  const match = new RegExp(`${selector.replace(".", "\\.")}\\s*\\{([^}]*)\\}`).exec(css);
  if (match === null || match[1] === undefined) {
    throw new Error(`global.css: expected a \`${selector}\` block defining the ${selector === ":root" ? "light" : "dark"} palette`);
  }
  return match[1];
}

function parseVars(block: string): Map<string, string> {
  const vars = new Map<string, string>();
  const re = /--nuomi-([a-z-]+)\s*:\s*([^;]+);/g;
  for (const match of block.matchAll(re)) {
    const name = match[1];
    const value = match[2];
    if (name !== undefined && value !== undefined) vars.set(name.trim(), value.trim());
  }
  return vars;
}

const HEX_RE = /^#([0-9a-f]{6}|[0-9a-f]{3})$/i;
const RGBA_RE = /^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})\s*(?:,\s*(\d*\.?\d+)\s*)?\)$/i;

function parseColor(raw: string): Rgba {
  const hex = HEX_RE.exec(raw);
  if (hex !== null && hex[1] !== undefined) {
    const digits =
      hex[1].length === 3
        ? [...hex[1]].map((c) => `${c}${c}`).join("")
        : hex[1];
    return {
      r: Number.parseInt(digits.slice(0, 2), 16),
      g: Number.parseInt(digits.slice(2, 4), 16),
      b: Number.parseInt(digits.slice(4, 6), 16),
      a: 1,
    };
  }
  const fn = RGBA_RE.exec(raw);
  if (fn !== null && fn[1] !== undefined && fn[2] !== undefined && fn[3] !== undefined) {
    return { r: Number(fn[1]), g: Number(fn[2]), b: Number(fn[3]), a: fn[4] === undefined ? 1 : Number(fn[4]) };
  }
  throw new Error(`global.css: unsupported color value "${raw}" (need #hex or rgba())`);
}

function toCamel(varName: string): string {
  return varName.replace(/-([a-z])/g, (_m, c: string) => c.toUpperCase());
}

function resolveTheme(css: string, selector: string): ResolvedTheme {
  const vars = parseVars(extractBlock(css, selector));
  const resolved = {} as Record<string, Rgba>;
  for (const token of TOKEN_VARS) {
    const raw = vars.get(token);
    if (raw === undefined) {
      throw new Error(`global.css \`${selector}\`: missing \`--nuomi-${token}\` (contrast guard needs all tokens)`);
    }
    resolved[toCamel(token)] = parseColor(raw);
  }
  return resolved as unknown as ResolvedTheme;
}

function compositeOver(top: Rgba, bottom: Rgb): Rgb {
  if (top.a >= 1) return { r: top.r, g: top.g, b: top.b };
  return {
    r: Math.round(top.a * top.r + (1 - top.a) * bottom.r),
    g: Math.round(top.a * top.g + (1 - top.a) * bottom.g),
    b: Math.round(top.a * top.b + (1 - top.a) * bottom.b),
  };
}

function linearize(channel: number): number {
  const c = channel / 255;
  return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

function luminance(color: Rgb): number {
  return 0.2126 * linearize(color.r) + 0.7152 * linearize(color.g) + 0.0722 * linearize(color.b);
}

export function contrastRatio(fg: Rgb, bg: Rgb): number {
  const lf = luminance(fg);
  const lb = luminance(bg);
  const [hi, lo] = lf >= lb ? [lf, lb] : [lb, lf];
  return (hi + 0.05) / (lo + 0.05);
}

interface ContrastPair {
  name: string;
  threshold: number;
  rationale: string;
  fg: (t: ResolvedTheme) => Rgb;
  bg: (t: ResolvedTheme) => Rgb;
}

const PAIRS: ContrastPair[] = [
  // ── Text ≥ 4.5:1 ─────────────────────────────────────────────────────────
  { name: "text: ink on surface", threshold: TEXT_MIN, rationale: "Shell.tsx:19 app frame", fg: (t) => t.ink, bg: (t) => t.surface },
  { name: "text: ink on surface-raised", threshold: TEXT_MIN, rationale: "Bubble.tsx:27 message body", fg: (t) => t.ink, bg: (t) => t.surfaceRaised },
  { name: "text: ink on surface-overlay", threshold: TEXT_MIN, rationale: "FileTabs.tsx:33 active tab", fg: (t) => t.ink, bg: (t) => t.surfaceOverlay },
  { name: "text: ink-muted on surface", threshold: TEXT_MIN, rationale: "AsyncBoundary.tsx:24 empty/loading copy", fg: (t) => t.inkMuted, bg: (t) => t.surface },
  { name: "text: ink-muted on surface-raised", threshold: TEXT_MIN, rationale: "TimelineRow.tsx:28 tool payload pre", fg: (t) => t.inkMuted, bg: (t) => t.surfaceRaised },
  { name: "text: ink-muted on surface-overlay", threshold: TEXT_MIN, rationale: "BoardColumn.tsx:88 count chip", fg: (t) => t.inkMuted, bg: (t) => t.surfaceOverlay },
  { name: "text: accent on surface", threshold: TEXT_MIN, rationale: "ScheduleRow.tsx:19 cron mono", fg: (t) => t.accent, bg: (t) => t.surface },
  { name: "text: accent on surface-raised", threshold: TEXT_MIN, rationale: "Bubble.tsx:30 speaker name", fg: (t) => t.accent, bg: (t) => t.surfaceRaised },
  { name: "text: accent on surface-overlay", threshold: TEXT_MIN, rationale: "FileTree.tsx:93 active row", fg: (t) => t.accent, bg: (t) => t.surfaceOverlay },
  { name: "button: surface text on solid accent", threshold: TEXT_MIN, rationale: "BoardView.tsx:138 primary buttons", fg: (t) => t.surface, bg: (t) => t.accent },
  { name: "button: surface text on solid ok", threshold: TEXT_MIN, rationale: "ApprovalRow.tsx:25 approve", fg: (t) => t.surface, bg: (t) => t.ok },
  { name: "chip: surface text on solid danger", threshold: TEXT_MIN, rationale: "BoardColumn.tsx:63 card delete", fg: (t) => t.surface, bg: (t) => t.danger },
  {
    name: "badge: ok on ok/20 over surface-raised",
    threshold: TEXT_MIN,
    rationale: "ScheduleRow.tsx:35 · OnlineAuthToggle.tsx:46 · AutoFormConfirmDialog.tsx:110",
    fg: (t) => t.ok,
    bg: (t) => compositeOver({ ...t.ok, a: 0.2 }, t.surfaceRaised),
  },
  { name: "text: danger on surface", threshold: TEXT_MIN, rationale: "FileTabs.tsx:56 · ErrorBoundary.tsx:38", fg: (t) => t.danger, bg: (t) => t.surface },
  { name: "text: danger on surface-raised", threshold: TEXT_MIN, rationale: "TaskCard.tsx:150 menu · ApprovalRow.tsx:33 deny", fg: (t) => t.danger, bg: (t) => t.surfaceRaised },
  { name: "text: danger on surface-overlay", threshold: TEXT_MIN, rationale: "IntegrationsSection.tsx:88 hover", fg: (t) => t.danger, bg: (t) => t.surfaceOverlay },
  { name: "text: warn on surface", threshold: TEXT_MIN, rationale: "RunDrawer.tsx:52 running status · TimelineRow.tsx:44", fg: (t) => t.warn, bg: (t) => t.surface },
  {
    name: "badge: accent on accent/20 over surface-raised",
    threshold: TEXT_MIN,
    rationale: "RolesSection.tsx:33 · IntegrationsSection.tsx:64 · SettingsView.tsx:49 (raised parent is worst case)",
    fg: (t) => t.accent,
    bg: (t) => compositeOver({ ...t.accent, a: 0.2 }, t.surfaceRaised),
  },
  {
    name: "bubble: ink on accent/20 over surface-raised",
    threshold: TEXT_MIN,
    rationale: "Bubble.tsx:27 user message",
    fg: (t) => t.ink,
    bg: (t) => compositeOver({ ...t.accent, a: 0.2 }, t.surfaceRaised),
  },
  {
    name: "backdrop: ink over scrim composited on surface",
    threshold: TEXT_MIN,
    rationale: "RunDrawer.tsx:21 · AutoFormConfirmDialog.tsx:74 scrim rule",
    fg: (t) => t.ink,
    bg: (t) => compositeOver(t.scrim, t.surface),
  },
  // ── Non-text UI components ≥ 3:1 (WCAG 1.4.11) ──────────────────────────
  { name: "border: ink-muted vs surface", threshold: UI_MIN, rationale: "NewTaskForm.tsx:67 bare-border buttons", fg: (t) => t.inkMuted, bg: (t) => t.surface },
  { name: "border: ink-muted vs surface-raised", threshold: UI_MIN, rationale: "AsyncBoundary.tsx:38 retry · Spinner.tsx:9 · MonacoTab.tsx:68", fg: (t) => t.inkMuted, bg: (t) => t.surfaceRaised },
  { name: "border: ok vs surface-overlay", threshold: UI_MIN, rationale: "Toaster.tsx:20 success edge", fg: (t) => t.ok, bg: (t) => t.surfaceOverlay },
  { name: "border: danger vs surface-overlay", threshold: UI_MIN, rationale: "Toaster.tsx:19 error edge", fg: (t) => t.danger, bg: (t) => t.surfaceOverlay },
  { name: "dot: ok vs surface-raised", threshold: UI_MIN, rationale: "AreaNav.tsx status dot connected", fg: (t) => t.ok, bg: (t) => t.surfaceRaised },
  { name: "dot: warn vs surface-raised", threshold: UI_MIN, rationale: "AreaNav.tsx status dot connecting", fg: (t) => t.warn, bg: (t) => t.surfaceRaised },
  { name: "dot: danger vs surface-raised", threshold: UI_MIN, rationale: "AreaNav.tsx status dot disconnected", fg: (t) => t.danger, bg: (t) => t.surfaceRaised },
  { name: "ring/border: accent vs surface-raised", threshold: UI_MIN, rationale: "BoardColumn.tsx:44 drag-over · focus rings", fg: (t) => t.accent, bg: (t) => t.surfaceRaised },
  { name: "ring: accent vs surface", threshold: UI_MIN, rationale: "focus-visible rings on page bg", fg: (t) => t.accent, bg: (t) => t.surface },
];

const THEMES: ReadonlyArray<{ name: "light" | "dark"; selector: string }> = [
  { name: "light", selector: ":root" },
  { name: "dark", selector: ".dark" },
];

describe("theme contrast guard (WCAG via parsed global.css palettes)", () => {
  const cssPromise = readGlobalCss();
  const resolvedByTheme = new Map<string, ResolvedTheme>();

  async function paletteOf(name: "light" | "dark"): Promise<ResolvedTheme> {
    const cached = resolvedByTheme.get(name);
    if (cached !== undefined) return cached;
    const theme = THEMES.find((t) => t.name === name);
    if (theme === undefined) throw new Error(`unknown theme ${name}`);
    const resolved = resolveTheme(await cssPromise, theme.selector);
    resolvedByTheme.set(name, resolved);
    return resolved;
  }

  for (const theme of THEMES) {
    describe(`${theme.name} palette (:root → ${theme.selector})`, () => {
      for (const pair of PAIRS) {
        it(`${pair.name} ≥ ${pair.threshold}:1  [${pair.rationale}]`, async () => {
          const resolved = await paletteOf(theme.name);
          const ratio = contrastRatio(pair.fg(resolved), pair.bg(resolved));
          expect(
            ratio,
            `${theme.name} · ${pair.name} measures ${ratio.toFixed(2)}:1 — needs ≥ ${pair.threshold}:1. Tune --nuomi-* in src/styles/global.css.`,
          ).toBeGreaterThanOrEqual(pair.threshold);
        });
      }
    });
  }
});
