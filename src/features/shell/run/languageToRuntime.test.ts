import { describe, expect, it } from "vitest";
import type { SandboxRuntimeDto } from "../../../lib/ipc/bindings.gen";
import { languageToRuntime } from "./languageToRuntime";

function runtime(id: string): SandboxRuntimeDto {
  return { id, name: id, isolation: "container" };
}

const ALL_RUNTIMES: SandboxRuntimeDto[] = [
  runtime("python"),
  runtime("node"),
  runtime("rust"),
  runtime("go"),
  runtime("java"),
  runtime("c"),
  runtime("cpp"),
];

describe("languageToRuntime", () => {
  it("maps each supported language to its runtime id", () => {
    expect(languageToRuntime("python", ALL_RUNTIMES)).toBe("python");
    expect(languageToRuntime("javascript", ALL_RUNTIMES)).toBe("node");
    expect(languageToRuntime("typescript", ALL_RUNTIMES)).toBe("node");
    expect(languageToRuntime("rust", ALL_RUNTIMES)).toBe("rust");
    expect(languageToRuntime("go", ALL_RUNTIMES)).toBe("go");
    expect(languageToRuntime("java", ALL_RUNTIMES)).toBe("java");
    expect(languageToRuntime("c", ALL_RUNTIMES)).toBe("c");
    expect(languageToRuntime("cpp", ALL_RUNTIMES)).toBe("cpp");
  });

  it("returns null for unsupported languages", () => {
    expect(languageToRuntime("markdown", ALL_RUNTIMES)).toBeNull();
    expect(languageToRuntime("plaintext", ALL_RUNTIMES)).toBeNull();
    expect(languageToRuntime("yaml", ALL_RUNTIMES)).toBeNull();
  });

  it("returns null when the mapped runtime is not registered", () => {
    expect(languageToRuntime("python", [runtime("node"), runtime("rust")])).toBeNull();
    expect(languageToRuntime("rust", [runtime("python")])).toBeNull();
  });

  it("returns null for an empty runtimes list", () => {
    expect(languageToRuntime("python", [])).toBeNull();
    expect(languageToRuntime("typescript", [])).toBeNull();
  });
});
