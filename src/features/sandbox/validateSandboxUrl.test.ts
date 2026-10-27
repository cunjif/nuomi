import { describe, expect, it } from "vitest";
import { validateSandboxUrl } from "./validateSandboxUrl";

describe("validateSandboxUrl", () => {
  it("accepts a legal http URL", () => {
    expect(validateSandboxUrl("http://127.0.0.1:8819")).toEqual({ valid: true });
  });

  it("accepts a legal https URL", () => {
    expect(validateSandboxUrl("https://example.com")).toEqual({ valid: true });
  });

  it("rejects a file:// URL", () => {
    const r = validateSandboxUrl("file:///tmp");
    expect(r.valid).toBe(false);
    expect(r.reason).toMatch(/protocol/);
  });

  it("rejects an empty string", () => {
    const r = validateSandboxUrl("");
    expect(r.valid).toBe(false);
    expect(r.reason).toMatch(/empty/);
  });

  it("rejects an ftp:// URL", () => {
    const r = validateSandboxUrl("ftp://x");
    expect(r.valid).toBe(false);
    expect(r.reason).toMatch(/protocol/);
  });
});
