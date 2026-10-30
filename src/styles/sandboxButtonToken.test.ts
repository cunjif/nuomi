// Regression gate for the sandbox/git accent-button readability fix
// (spec §5.3 / design §2.5 anchors #3–#5, #9). The root cause was an invalid
// Tailwind class `text-surface-base` (the `surface` color group has no `base`
// sub-key), which collapsed the text color to `--nuomi-ink` and made the
// button label invisible under the eink-paper theme where `--nuomi-ink` and
// `--nuomi-accent` are both `#1a1a1a`. The fix swaps `text-surface-base` →
// `text-surface` (a valid token already guarded by themeContrast.test.ts:177
// across all five themes at ≥ 4.5:1).
//
// This test scans the three accent-button call sites and asserts each button
// line carries `text-surface` and never the invalid `text-surface-base`. It is
// a static className guard — complementary to themeContrast.test.ts (which
// guards the resolved color contrast) — so a future revert that reintroduces
// the invalid token fails here before it can reach a human eye.
import { describe, expect, it } from "vitest";

declare const process: { cwd(): string };

type FsLike = { readFileSync: (filePath: string, options: { encoding: "utf8" }) => string };

async function readSource(rel: string): Promise<string> {
  const builtinId = ["node", "fs"].join(":");
  const fs = (await import(/* @vite-ignore */ builtinId)) as unknown as FsLike;
  return fs.readFileSync(`${process.cwd()}/${rel}`, { encoding: "utf8" });
}

const BUTTON_FILES = [
  "src/features/settings/SandboxSection.tsx",
  "src/features/sandbox/SandboxSetupGuide.tsx",
  "src/features/git/AgentBindingGuide.tsx",
] as const;

describe("sandbox button className token guard", () => {
  for (const rel of BUTTON_FILES) {
    it(`${rel}: accent buttons use text-surface (not the invalid text-surface-base)`, async () => {
      const src = await readSource(rel);
      const buttonLines = src
        .split(/\r?\n/)
        .filter((line) => line.includes("bg-ink-accent") && line.includes("className"));
      expect(buttonLines.length).toBeGreaterThan(0);
      for (const line of buttonLines) {
        expect(line).toContain("text-surface");
        expect(line).not.toContain("text-surface-base");
      }
    });
  }
});
