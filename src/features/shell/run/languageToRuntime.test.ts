import { describe, expect, it } from "vitest";
import { languageToRunCommand, detectBuildCommand } from "./languageToRuntime";
import { DEFAULT_SDK_CONFIGS, type SdkConfigs } from "./sdkConfigs";

describe("languageToRunCommand", () => {
  it("maps each supported language to a terminal command using defaults", () => {
    expect(languageToRunCommand("python", "main.py")).toBe('python "main.py"');
    expect(languageToRunCommand("javascript", "index.js")).toBe('node "index.js"');
    expect(languageToRunCommand("typescript", "app.ts")).toBe('npx tsx "app.ts"');
    expect(languageToRunCommand("go", "main.go")).toBe('go run "main.go"');
  });

  it("supports rust, java, c, cpp, shell with default configs", () => {
    expect(languageToRunCommand("rust", "main.rs")).toBe('cargo run "main.rs"');
    expect(languageToRunCommand("java", "Main.java")).toBe('java "Main.java"');
    expect(languageToRunCommand("shell", "script.sh")).toBe('bash "script.sh"');
  });

  it("expands ${file} placeholder in args for C/C++", () => {
    const cCmd = languageToRunCommand("c", "main.c");
    expect(cCmd).toContain('gcc');
    expect(cCmd).toContain('"main.c"');

    const cppCmd = languageToRunCommand("cpp", "main.cpp");
    expect(cppCmd).toContain('g++');
    expect(cppCmd).toContain('"main.cpp"');
  });

  it("returns null for unsupported languages", () => {
    expect(languageToRunCommand("markdown", "README.md")).toBeNull();
    expect(languageToRunCommand("plaintext", "notes.txt")).toBeNull();
    expect(languageToRunCommand("yaml", "config.yaml")).toBeNull();
  });

  it("returns null for disabled languages", () => {
    const configs: SdkConfigs = {
      languages: { ...DEFAULT_SDK_CONFIGS.languages, python: { command: "python", args: "", enabled: false } },
      builds: DEFAULT_SDK_CONFIGS.builds,
    };
    expect(languageToRunCommand("python", "main.py", configs)).toBeNull();
  });

  it("uses custom command from configs when provided", () => {
    const configs: SdkConfigs = {
      languages: { ...DEFAULT_SDK_CONFIGS.languages, python: { command: "python3", args: "-u", enabled: true } },
      builds: DEFAULT_SDK_CONFIGS.builds,
    };
    expect(languageToRunCommand("python", "main.py", configs)).toBe('python3 -u "main.py"');
  });
});

describe("detectBuildCommand", () => {
  it("detects npm project", () => {
    expect(detectBuildCommand(["package.json", "src", "node_modules"])).toBe("npm run build");
  });

  it("detects cargo project", () => {
    expect(detectBuildCommand(["Cargo.toml", "src"])).toBe("cargo build");
  });

  it("detects go project", () => {
    expect(detectBuildCommand(["go.mod", "main.go"])).toBe("go build");
  });

  it("detects maven project", () => {
    expect(detectBuildCommand(["pom.xml", "src"])).toBe("mvn package");
  });

  it("detects gradle project", () => {
    expect(detectBuildCommand(["build.gradle", "src"])).toBe("gradle build");
  });

  it("detects makefile project", () => {
    expect(detectBuildCommand(["Makefile", "src"])).toBe("make");
  });

  it("detects cmake project", () => {
    expect(detectBuildCommand(["CMakeLists.txt", "src"])).toBe("cmake --build build");
  });

  it("returns null for unknown project types", () => {
    expect(detectBuildCommand(["main.py", "utils.py"])).toBeNull();
    expect(detectBuildCommand([])).toBeNull();
  });

  it("skips disabled build types", () => {
    const configs: SdkConfigs = {
      languages: DEFAULT_SDK_CONFIGS.languages,
      builds: { ...DEFAULT_SDK_CONFIGS.builds, node: { manifestFile: "package.json", command: "npm run build", enabled: false } },
    };
    expect(detectBuildCommand(["package.json"], configs)).toBeNull();
  });

  it("uses custom build command from configs", () => {
    const configs: SdkConfigs = {
      languages: DEFAULT_SDK_CONFIGS.languages,
      builds: { ...DEFAULT_SDK_CONFIGS.builds, node: { manifestFile: "package.json", command: "pnpm build", enabled: true } },
    };
    expect(detectBuildCommand(["package.json"], configs)).toBe("pnpm build");
  });
});
