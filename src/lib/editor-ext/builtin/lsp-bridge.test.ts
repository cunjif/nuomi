import { describe, expect, it } from "vitest";
import { withTimeout } from "./lsp-bridge";

describe("lsp-bridge withTimeout", () => {
  it("returns the resolved value when the promise fulfills first", async () => {
    const result = await withTimeout(Promise.resolve("ok"), 1000);
    expect(result).toBe("ok");
  });

  it("returns null when the promise rejects (LS unavailable → fall back, no throw)", async () => {
    const result = await withTimeout(Promise.reject(new Error("jdtls: program not found")), 1000);
    expect(result).toBeNull();
  });

  it("returns null when the timeout fires before the promise resolves", async () => {
    const result = await withTimeout(new Promise<string>(() => {}), 10);
    expect(result).toBeNull();
  });
});
