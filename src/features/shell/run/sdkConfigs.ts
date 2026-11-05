/**
 * SDK configuration types and defaults for the run/build infrastructure.
 * Stored as a JSON blob in app_settings under the key "sdk_configs".
 * The frontend owns the schema; the backend just persists the string.
 */

/** Per-language SDK configuration: how to run a single file of this language. */
export interface SdkLanguageConfig {
  /** Base command to execute (e.g. "python", "node", "cargo run"). */
  command: string;
  /** Extra arguments inserted before the file path (e.g. "-o a.out && ./a.out" for C). */
  args: string;
  /** Whether this language's run button is active. */
  enabled: boolean;
  /** Command to run before the main run command (e.g. compile step). */
  preRunCommand?: string;
  /** Environment variables to set when running (KEY=VALUE pairs). */
  envVars?: Record<string, string>;
  /** Working directory for the run command (defaults to file's directory). */
  workingDir?: string;
  /** File extensions associated with this language. */
  fileExtensions?: string[];
}

/** Per-project-type build configuration: how to build a project of this type. */
export interface SdkBuildConfig {
  /** Manifest file that triggers this build detection (e.g. "package.json"). */
  manifestFile: string;
  /** Build command to execute (e.g. "npm run build"). */
  command: string;
  /** Whether this build type is active. */
  enabled: boolean;
  /** Extra arguments for the build command. */
  buildArgs?: string;
  /** Output directory for build artifacts. */
  outputDir?: string;
}

/** Full SDK configuration blob persisted in app_settings. */
export interface SdkConfigs {
  languages: Record<string, SdkLanguageConfig>;
  builds: Record<string, SdkBuildConfig>;
}

/** Default language SDK configurations — covers all mainstream languages. */
export const DEFAULT_LANGUAGE_CONFIGS: Record<string, SdkLanguageConfig> = {
  python: { command: "python", args: "", enabled: true, fileExtensions: [".py"] },
  javascript: { command: "node", args: "", enabled: true, fileExtensions: [".js", ".mjs", ".cjs"] },
  typescript: { command: "npx tsx", args: "", enabled: true, fileExtensions: [".ts", ".tsx"] },
  go: { command: "go run", args: "", enabled: true, fileExtensions: [".go"] },
  rust: { command: "cargo run", args: "", enabled: true, fileExtensions: [".rs"] },
  java: { command: "java", args: "", enabled: true, fileExtensions: [".java"] },
  c: { command: "gcc", args: '-o "${file}.out" && "./${file}.out"', enabled: true, fileExtensions: [".c"] },
  cpp: { command: "g++", args: '-o "${file}.out" && "./${file}.out"', enabled: true, fileExtensions: [".cpp", ".cc", ".cxx"] },
  shell: { command: "bash", args: "", enabled: true, fileExtensions: [".sh", ".bash"] },
};

/** Default build configurations — covers all mainstream project types. */
export const DEFAULT_BUILD_CONFIGS: Record<string, SdkBuildConfig> = {
  node: { manifestFile: "package.json", command: "npm run build", enabled: true, outputDir: "dist" },
  cargo: { manifestFile: "Cargo.toml", command: "cargo build", enabled: true, outputDir: "target/debug" },
  go: { manifestFile: "go.mod", command: "go build", enabled: true },
  maven: { manifestFile: "pom.xml", command: "mvn package", enabled: true, outputDir: "target" },
  gradle: { manifestFile: "build.gradle", command: "gradle build", enabled: true, outputDir: "build" },
  make: { manifestFile: "Makefile", command: "make", enabled: true },
  cmake: { manifestFile: "CMakeLists.txt", command: "cmake --build build", enabled: true, outputDir: "build" },
};

/** Default full SDK configs blob. */
export const DEFAULT_SDK_CONFIGS: SdkConfigs = {
  languages: DEFAULT_LANGUAGE_CONFIGS,
  builds: DEFAULT_BUILD_CONFIGS,
};

/** app_settings key for the SDK configs blob. */
export const SDK_CONFIGS_KEY = "sdk_configs";

/**
 * Parse a JSON string into SdkConfigs, falling back to defaults on any error
 * or missing keys. Merges with defaults so new languages/builds added in
 * future versions appear automatically.
 */
export function parseSdkConfigs(raw: string | null | undefined): SdkConfigs {
  if (raw === null || raw === undefined || raw.trim() === "") {
    return DEFAULT_SDK_CONFIGS;
  }
  try {
    const parsed = JSON.parse(raw) as Partial<SdkConfigs>;
    const languages = { ...DEFAULT_LANGUAGE_CONFIGS, ...(parsed.languages ?? {}) };
    const builds = { ...DEFAULT_BUILD_CONFIGS, ...(parsed.builds ?? {}) };
    return { languages, builds };
  } catch {
    return DEFAULT_SDK_CONFIGS;
  }
}

/**
 * Serialize SdkConfigs to a JSON string for persistence.
 */
export function serializeSdkConfigs(configs: SdkConfigs): string {
  return JSON.stringify(configs);
}
