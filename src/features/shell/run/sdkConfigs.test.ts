import { describe, expect, it } from "vitest";
import {
  DEFAULT_SDK_CONFIGS,
  DEFAULT_LANGUAGE_CONFIGS,
  DEFAULT_BUILD_CONFIGS,
  parseSdkConfigs,
  serializeSdkConfigs,
  SDK_CONFIGS_KEY,
} from "./sdkConfigs";

describe("DEFAULT_SDK_CONFIGS", () => {
  it("includes all mainstream languages", () => {
    const langs = Object.keys(DEFAULT_LANGUAGE_CONFIGS);
    expect(langs).toContain("python");
    expect(langs).toContain("javascript");
    expect(langs).toContain("typescript");
    expect(langs).toContain("go");
    expect(langs).toContain("rust");
    expect(langs).toContain("java");
    expect(langs).toContain("c");
    expect(langs).toContain("cpp");
    expect(langs).toContain("shell");
  });

  it("includes all mainstream build tools", () => {
    const builds = Object.keys(DEFAULT_BUILD_CONFIGS);
    expect(builds).toContain("node");
    expect(builds).toContain("cargo");
    expect(builds).toContain("go");
    expect(builds).toContain("maven");
    expect(builds).toContain("gradle");
    expect(builds).toContain("make");
    expect(builds).toContain("cmake");
  });

  it("all defaults are enabled", () => {
    for (const cfg of Object.values(DEFAULT_LANGUAGE_CONFIGS)) {
      expect(cfg.enabled).toBe(true);
    }
    for (const cfg of Object.values(DEFAULT_BUILD_CONFIGS)) {
      expect(cfg.enabled).toBe(true);
    }
  });
});

describe("parseSdkConfigs", () => {
  it("returns defaults for null/empty input", () => {
    expect(parseSdkConfigs(null)).toEqual(DEFAULT_SDK_CONFIGS);
    expect(parseSdkConfigs(undefined)).toEqual(DEFAULT_SDK_CONFIGS);
    expect(parseSdkConfigs("")).toEqual(DEFAULT_SDK_CONFIGS);
    expect(parseSdkConfigs("   ")).toEqual(DEFAULT_SDK_CONFIGS);
  });

  it("returns defaults for invalid JSON", () => {
    expect(parseSdkConfigs("{not valid json")).toEqual(DEFAULT_SDK_CONFIGS);
  });

  it("merges with defaults — new keys appear automatically", () => {
    const raw = JSON.stringify({
      languages: { python: { command: "python3", args: "", enabled: false } },
    });
    const result = parseSdkConfigs(raw);
    expect(result.languages.python?.command).toBe("python3");
    expect(result.languages.python?.enabled).toBe(false);
    expect(result.languages.javascript).toEqual(DEFAULT_LANGUAGE_CONFIGS.javascript);
    expect(result.builds).toEqual(DEFAULT_BUILD_CONFIGS);
  });

  it("preserves custom build configs", () => {
    const raw = JSON.stringify({
      builds: { node: { manifestFile: "package.json", command: "pnpm build", enabled: true } },
    });
    const result = parseSdkConfigs(raw);
    expect(result.builds.node?.command).toBe("pnpm build");
    expect(result.builds.cargo).toEqual(DEFAULT_BUILD_CONFIGS.cargo);
  });
});

describe("serializeSdkConfigs", () => {
  it("round-trips through parseSdkConfigs", () => {
    const serialized = serializeSdkConfigs(DEFAULT_SDK_CONFIGS);
    const parsed = parseSdkConfigs(serialized);
    expect(parsed).toEqual(DEFAULT_SDK_CONFIGS);
  });
});

describe("SDK_CONFIGS_KEY", () => {
  it("is the app_settings key", () => {
    expect(SDK_CONFIGS_KEY).toBe("sdk_configs");
  });
});
