import { describe, it, expect } from "vitest";
import { workspaceBadgeColor } from "./workspaceBadgeColor";

describe("workspaceBadgeColor", () => {
  it("returns the same color for the same workspace id (deterministic)", () => {
    const id = "019a7f3c-2b1e-4c8d-9f2a-6e5d4c3b2a1f";
    expect(workspaceBadgeColor(id)).toBe(workspaceBadgeColor(id));
  });

  it("returns a valid hex color", () => {
    const color = workspaceBadgeColor("ws-1");
    expect(color).toMatch(/^#[0-9a-f]{6}$/);
  });

  it("distinguishes different workspaces (at least 2 distinct colors in a sample)", () => {
    const ids = ["ws-alpha", "ws-beta", "ws-gamma", "ws-delta", "ws-epsilon", "ws-zeta", "ws-eta", "ws-theta"];
    const colors = new Set(ids.map(workspaceBadgeColor));
    expect(colors.size).toBeGreaterThan(1);
  });

  it("handles the empty string without throwing", () => {
    const color = workspaceBadgeColor("");
    expect(color).toMatch(/^#[0-9a-f]{6}$/);
  });
});
