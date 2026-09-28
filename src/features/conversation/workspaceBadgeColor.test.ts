import { describe, it, expect } from "vitest";
import { workspaceBadgeColor } from "./workspaceBadgeColor";

describe("workspaceBadgeColor", () => {
  it("returns the same color for the same path (deterministic)", () => {
    const path = "C:\\Users\\james\\Codehub\\nuomi";
    expect(workspaceBadgeColor(path)).toBe(workspaceBadgeColor(path));
  });

  it("returns a valid hex color", () => {
    const color = workspaceBadgeColor("/some/path");
    expect(color).toMatch(/^#[0-9a-f]{6}$/);
  });

  it("distinguishes different paths (at least 2 distinct colors in a sample)", () => {
    const paths = [
      "C:\\work\\alpha",
      "C:\\work\\beta",
      "C:\\work\\gamma",
      "C:\\work\\delta",
      "C:\\work\\epsilon",
      "C:\\work\\zeta",
      "C:\\work\\eta",
      "C:\\work\\theta",
    ];
    const colors = new Set(paths.map(workspaceBadgeColor));
    expect(colors.size).toBeGreaterThan(1);
  });

  it("handles empty path without throwing", () => {
    const color = workspaceBadgeColor("");
    expect(color).toMatch(/^#[0-9a-f]{6}$/);
  });
});
